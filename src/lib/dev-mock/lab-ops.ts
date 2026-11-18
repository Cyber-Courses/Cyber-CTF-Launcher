// The lab operations in the browser preview: labs started or stopped here stay so until a reload.
// Docker labs stream a Compose log, VM labs a Vagrant log (on this machine's hypervisor or a saved
// server; pause, shut down, resume and re-run provisioning work), cloud labs a Terraform log
// (outputs and an auto-stop).
import { GB } from "@/lib/format";
import { argText, fail, send, sleep } from "@/lib/dev-mock/command";
import { hostsOf } from "@/lib/dev-mock/hosts";
import { type BrowserHandler, type Context, type Live, labArg, labRuntime, operation, slugOf, statusOf, vagrantMachines } from "@/lib/dev-mock/live";
import { answer } from "@/lib/dev-mock/sample";
import type { Args } from "@/lib/dev-mock/types";
import { SAMPLE_IN_FLIGHT } from "@/lib/dev-mock/labs";
import type { ActiveOperation } from "@/lib/tauri/run-events";
import type { LabPlacement } from "@/lib/tauri";
import type { DeployStep } from "@/lib/tauri/generated";

/** What a start streams: a log line, or a step the runner says (`isothrum_runner::Phase`'s id),
 *  as the core relays both. The lab page's steps are the runner's alone. */
type Said = string | DeployStep;

/** Step `step` in `state`: about machine `label` when it is one of several, `done` of `total`
 *  items when it counts them. */
const step = (id: string, state: DeployStep["state"], label: string | null = null, done?: number, total?: number): DeployStep => ({
  step: id,
  state,
  label,
  count: done === undefined ? null : { done, total: total ?? null },
});

/** A Docker Compose start, as the launcher streams it. */
const composeUp = (slug: string): Said[] => [
  `Downloading ${slug}…`,
  "Lab installed",
  step("images", "started", null, 0, 1),
  ` Image ${slug}-web Pulling`,
  ` Image ${slug}-web Pulled`,
  step("images", "done", null, 1, 1),
  step("create", "started", null, 0, 2),
  ` Network ${slug}_default Creating`,
  ` Network ${slug}_default Created`,
  ` Container ${slug}-web-1 Creating`,
  ` Container ${slug}-web-1 Created`,
  step("create", "done", null, 2, 2),
  step("start", "started", null, 0, 1),
  ` Container ${slug}-web-1 Starting`,
  ` Container ${slug}-web-1 Started`,
  step("start", "done", null, 1, 1),
  step("health", "started", null, 0, 1),
  ` Container ${slug}-web-1 Waiting`,
  ` Container ${slug}-web-1 Healthy`,
  step("health", "done", null, 1, 1),
];

/** A cloud lab's Terraform apply. */
const terraformApply = (slug: string): Said[] => [
  `Downloading ${slug}…`,
  "Lab installed",
  step("init", "started"),
  "Initializing the backend...",
  "Initializing provider plugins...",
  step("init", "done"),
  step("apply", "started", null, 0, 6),
  "Terraform will perform the following actions:",
  "Plan: 6 to add, 0 to change, 0 to destroy.",
  "aws_instance.web: Creating...",
  "aws_instance.web: Still creating... [10s elapsed]",
  "aws_instance.web: Creation complete after 21s [id=i-0c0ffee5afe]",
  "Apply complete! Resources: 6 added, 0 changed, 0 destroyed.",
  step("apply", "done", null, 6, 6),
];

/** A VM lab's `vagrant up` with `provider`, on the server `hostName` or here (null): its box,
 *  then each machine's start and provisioning, a step of its own each. */
const vagrantUp = (slug: string, provider: string, hostName: string | null): Said[] => [
  `Downloading ${slug}…`,
  "Lab installed",
  ...(hostName ? [`Running on server host ${hostName}`] : []),
  ...vagrantMachines().map((m) => `Bringing machine '${m}' up with '${provider}' provider...`),
  step("boxes", "started", null, 0, 1),
  `==> ${vagrantMachines()[0] ?? "default"}: Importing base box 'StefanScherer/windows_2019'...`,
  step("boxes", "done", null, 1, 1),
  ...vagrantMachines().flatMap((m) => [
    step("up", "started", m),
    `==> ${m}: Booting VM...`,
    `==> ${m}: Machine booted and ready!`,
    step("up", "done", m),
    step("provision", "started", m),
    `==> ${m}: Running provisioner: ansible...`,
    step("provision", "done", m),
  ]),
];

/** `said` up to its `lines`-th log line, the steps among them kept: what a start that fails
 *  then streamed. */
const upTo = (said: Said[], lines: number): Said[] => {
  let seen = 0;
  return said.filter((s) => (typeof s === "string" ? ++seen <= lines : seen < lines));
};

/** A container lab started here, on its own host ports or random ones (`defaultPorts`); with
 *  `launch=fail`, the first one fails on its port. With
 *  `hold=launch`, it stays starting, its images pulled and its containers created but not started,
 *  until the test lets it go on. Started again offline (after a launch failed with `offlineReady`),
 *  its log says so first, as the core's does. */
async function launchContainer(id: string, args: Args, live: Live) {
  const slug = slugOf(id);
  live.ops.set(id, "launch");
  if (args?.defaultPorts === true) live.ownPorts.add(id);
  else live.ownPorts.delete(id);
  try {
    if (args?.offline === true) send(args, "Starting offline: Cyber CTF can't be reached, progress isn't saved");
    for (const line of composeUp(slug)) {
      // Before the containers start: once one runs, the lab's status reads as running.
      const starting = typeof line === "string" && line.endsWith(" Starting");
      if (starting) await live.holds.passed("launch");
      if (live.launchFails && starting) {
        live.launchFails = false;
        await sleep(120);
        send(args, step("start", "failed", null, 0, 1));
        fail(
          `Error response from daemon: driver failed programming external connectivity on endpoint ${slug}-web-1: Bind for 0.0.0.0:8443 failed: port is already allocated`,
        );
      }
      send(args, line);
      // The containers run once started, before the launch is over (it waits until healthy).
      if (typeof line === "string" && line.endsWith(" Started")) live.running.add(id);
      await sleep(120);
    }
    return null;
  } finally {
    live.ops.delete(id);
  }
}

/** A VM lab started with Vagrant (on this machine's hypervisor, or a saved server), or a cloud lab
 *  with Terraform in a saved cloud account. */
async function launchElsewhere(id: string, runtime: "VM" | "CLOUD", args: Args, { sc, live }: Context) {
  const host = args?.host ? (hostsOf(sc).find((h) => h.id === args.host) ?? null) : null;
  if (args?.host && !host) fail(`no saved server or cloud account '${argText(args.host)}'`);
  // Nowhere to start it, refused as the backend's `check_destination` refuses it.
  if (!args?.host && runtime === "CLOUD") fail("This lab runs in your cloud account: pick one of your cloud accounts to start it on.", "pickCloudAccount");
  if (!args?.host && !args?.provider && runtime === "VM") fail("No hypervisor ready on this machine runs this lab.", "noHypervisor");
  const provider = argText(host?.provider, argText(args?.provider, "virtualbox"));
  const cloud = runtime === "CLOUD";
  const lines = cloud ? terraformApply(slugOf(id)) : vagrantUp(slugOf(id), provider, host ? argText(host.name) : null);
  if (live.launchFails) {
    live.launchFails = false;
    await operation({ id, op: "launch", lines: upTo(lines, 4) }, args, live);
    send(args, cloud ? step("apply", "failed", null, 0, 6) : step("up", "failed", vagrantMachines()[0] ?? null));
    fail(
      cloud
        ? "Error: creating EC2 Instance: UnauthorizedOperation: You are not authorized to perform this operation."
        : "The guest machine entered an invalid state while waiting for it to boot (while executing the action on the 'dc01' machine)",
    );
  }
  await operation({ id, op: "launch", lines }, args, live);
  live.placed.set(id, { provider, host, parked: null, startedAt: Date.now() });
  live.running.add(id);
  return null;
}

/** What a lab's stop streams, by how it runs. */
function stopLines(id: string): string[] {
  const runtime = labRuntime(id);
  if (runtime === "VM") return vagrantMachines().flatMap((m) => [`==> ${m}: Forcing shutdown of VM...`, `==> ${m}: Destroying VM and associated drives...`]);
  if (runtime === "CLOUD")
    return [
      "aws_instance.web: Destroying... [id=i-0c0ffee5afe]",
      "aws_instance.web: Destruction complete after 31s",
      "Destroy complete! Resources: 6 destroyed.",
    ];
  const slug = slugOf(id);
  return [` Container ${slug}-web-1 Stopping`, ` Container ${slug}-web-1 Stopped`, ` Container ${slug}-web-1 Removed`];
}

/** A launch's placement (`lab_launch`'s, as the backend takes it) as the preview's launch reads
 *  it: the host, the hypervisor, one VM per machine, the lab's own ports. */
function flatPlacement(args: Args): Args {
  const p = (args?.placement ?? { kind: "docker" }) as LabPlacement;
  const host = p.kind === "server" ? p.host : p.kind === "cloud" ? p.account : null;
  const provider = p.kind === "vms" ? p.provider : null;
  const perMachineVm = p.kind === "vms" || (p.kind === "cloud" && p.perMachine);
  return { ...args, host, provider, perMachineVm, defaultPorts: p.kind === "docker" && p.defaultPorts };
}

const launch: BrowserHandler = (placed, ctx) => {
  const args = flatPlacement(placed);
  const id = labArg(args, "labId");
  ctx.live.here.add(id);
  const runtime = labRuntime(id);
  // A container lab picked as one VM per machine starts as a VM lab would.
  if (runtime === "DOCKER" && args?.perMachineVm === true) return launchElsewhere(id, "VM", args, ctx);
  return runtime === "VM" || runtime === "CLOUD" ? launchElsewhere(id, runtime, args, ctx) : launchContainer(id, args, ctx.live);
};

const stop: BrowserHandler = async (args, { live }) => {
  const id = labArg(args);
  await operation({ id, op: "stop", lines: stopLines(id) }, args, live);
  live.running.delete(id);
  live.placed.delete(id);
  return null;
};

const park: BrowserHandler = async (args, { live }) => {
  const id = labArg(args);
  const placed = live.placed.get(id);
  if (!placed) return fail("the lab isn't running");
  const mode = args?.mode === "pause" ? "pause" : "shutdown";
  const verb = mode === "pause" ? "Saving VM state and suspending execution..." : "Attempting graceful shutdown of VM...";
  await operation({ id, op: mode, lines: vagrantMachines().map((m) => `==> ${m}: ${verb}`) }, args, live);
  placed.parked = mode;
  live.running.delete(id);
  return null;
};

const resume: BrowserHandler = async (args, { live }) => {
  const id = labArg(args);
  const placed = live.placed.get(id);
  if (!placed?.parked) return fail("the lab isn't parked");
  const back = placed.parked === "pause" ? "Resuming suspended VM..." : "Booting VM...";
  await operation({ id, op: "resume", lines: vagrantMachines().flatMap((m) => [`==> ${m}: ${back}`, `==> ${m}: Machine booted and ready!`]) }, args, live);
  placed.parked = null;
  live.running.add(id);
  return null;
};

const provision: BrowserHandler = async (args, { live }) => {
  const id = labArg(args);
  if (!live.placed.get(id) || !live.running.has(id)) return fail("the lab isn't running");
  const machines = vagrantMachines(args?.machine ? argText(args.machine) : null);
  const lines = machines.flatMap((m) => [`==> ${m}: Running provisioner: ansible...`, `PLAY RECAP *********`, `${m} : ok=41 changed=3 unreachable=0 failed=0`]);
  await operation({ id, op: "provision", lines }, args, live);
  return null;
};

/** The scan follows the labs started and stopped here: a started container lab shows up in it
 *  (the sidebar lists running labs from it), a stopped one leaves it. */
const workloads: BrowserHandler = (args, { live }) => {
  const fixed = answer("machine_workloads", args) as { id: string; kind: string }[];
  const kept = fixed.filter((w) => w.kind !== "docker" || live.running.has(w.id));
  const started = [...live.running].filter((id) => !fixed.some((w) => w.id === id) && !live.placed.has(id));
  return [...kept, ...started.map((id) => ({ id, kind: "docker", count: 1, memBytes: 0.3 * GB, provider: null }))];
};

/** A lab removed from this computer, refused as the core refuses it while something of it is up
 *  or an operation runs on it; with `remove=partial`, one of its files stays. */
const remove: BrowserHandler = (args, { sc, live }) => {
  const id = labArg(args);
  if (!live.here.has(id)) fail(`lab \`${id}\` is not installed`);
  if (live.ops.has(id)) fail("Another operation on this lab is already in progress; wait for it to finish.", "alreadyInProgress");
  if (live.running.has(id) || live.placed.has(id))
    fail("This lab is still running on this machine (or its machines are still there). Stop it, then try again.", "labRunningHere");
  live.here.delete(id);
  live.imported = live.imported.filter((l) => l.id !== id);
  const left = sc.removeLeaves
    ? [{ path: `~/Library/Application Support/com.cyberctf.desktop/runs/cyberctf-${id}`, error: "Resource busy (os error 16)" }]
    : [];
  return { left };
};

/** The lab commands: their status, what runs and deploys, and the operations on them. */
export const labHandlers: Record<string, BrowserHandler> = {
  lab_status: (args, { live }) => statusOf(labArg(args), live),
  running_labs: (_args, { live }) => [...live.running],
  machine_workloads: workloads,
  busy_labs: (args, { sc, live }) => [...new Set([...(sc.noDeploying ? [] : (answer("busy_labs", args) as string[])), ...live.ops.keys()])],
  lab_launch: launch,
  lab_stop: stop,
  lab_park: park,
  lab_resume: resume,
  lab_provision: provision,
  imported_labs: (_args, { live }) => live.imported,
  lab_installed: (args, { live }) => live.here.has(labArg(args)),
  lab_remove: remove,
};

/** The operations the preview's core runs when a window replays its events: the sample's (unless
 *  `noDeploying`), and those the preview started. */
export function inFlight({ sc, live }: Context): ActiveOperation[] {
  const started = [...live.ops].map(([labId, op]): ActiveOperation => ({ labId, op, machine: null, deploying: true, step: null }));
  return [...(sc.noDeploying ? [] : SAMPLE_IN_FLIGHT), ...started];
}
