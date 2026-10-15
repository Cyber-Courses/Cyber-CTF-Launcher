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

/** A Docker Compose start, as the launcher streams it. */
const composeUp = (slug: string) => [
  `Downloading ${slug}…`,
  "Lab installed",
  ` Image ${slug}-web Pulling`,
  ` Image ${slug}-web Pulled`,
  ` Network ${slug}_default Creating`,
  ` Network ${slug}_default Created`,
  ` Container ${slug}-web-1 Creating`,
  ` Container ${slug}-web-1 Created`,
  ` Container ${slug}-web-1 Starting`,
  ` Container ${slug}-web-1 Started`,
  ` Container ${slug}-web-1 Waiting`,
  ` Container ${slug}-web-1 Healthy`,
];

/** A cloud lab's Terraform apply. */
const terraformApply = (slug: string) => [
  `Downloading ${slug}…`,
  "Lab installed",
  "Initializing the backend...",
  "Initializing provider plugins...",
  "Terraform will perform the following actions:",
  "Plan: 6 to add, 0 to change, 0 to destroy.",
  "aws_instance.web: Creating...",
  "aws_instance.web: Still creating... [10s elapsed]",
  "aws_instance.web: Creation complete after 21s [id=i-0c0ffee5afe]",
  "Apply complete! Resources: 6 added, 0 changed, 0 destroyed.",
];

/** A VM lab's `vagrant up` with `provider`, on the server `hostName` or here (null). */
const vagrantUp = (slug: string, provider: string, hostName: string | null) => [
  `Downloading ${slug}…`,
  "Lab installed",
  ...(hostName ? [`Running on server host ${hostName}`] : []),
  ...vagrantMachines().map((m) => `Bringing machine '${m}' up with '${provider}' provider...`),
  ...vagrantMachines().flatMap((m) => [
    `==> ${m}: Importing base box 'StefanScherer/windows_2019'...`,
    `==> ${m}: Booting VM...`,
    `==> ${m}: Machine booted and ready!`,
    `==> ${m}: Running provisioner: ansible...`,
  ]),
];

/** A container lab started here; with `launch=fail`, the first one fails on its port. */
async function launchContainer(id: string, args: Args, live: Live) {
  const slug = slugOf(id);
  live.ops.set(id, "launch");
  try {
    for (const [n, line] of composeUp(slug).entries()) {
      if (live.launchFails && n === 8) {
        live.launchFails = false;
        await sleep(120);
        fail(
          `Error response from daemon: driver failed programming external connectivity on endpoint ${slug}-web-1: Bind for 0.0.0.0:8443 failed: port is already allocated`,
        );
      }
      send(args, line);
      // The containers run once started, before the launch is over (it waits until healthy).
      if (line.endsWith(" Started")) live.running.add(id);
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
  const provider = argText(host?.provider, argText(args?.provider, "virtualbox"));
  const cloud = runtime === "CLOUD";
  const lines = cloud ? terraformApply(slugOf(id)) : vagrantUp(slugOf(id), provider, host ? argText(host.name) : null);
  if (live.launchFails) {
    live.launchFails = false;
    await operation({ id, op: "launch", lines: lines.slice(0, 4) }, args, live);
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

const launch: BrowserHandler = (args, ctx) => {
  const id = labArg(args, "labId");
  const runtime = labRuntime(id);
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

/** The lab commands: their status, what runs and deploys, and the operations on them. */
export const labHandlers: Record<string, BrowserHandler> = {
  lab_status: (args, { live }) => statusOf(labArg(args), live),
  running_labs: (_args, { live }) => [...live.running],
  machine_workloads: workloads,
  deploying_labs: (args, { sc, live }) => [...new Set([...(sc.noDeploying ? [] : (answer("deploying_labs", args) as string[])), ...live.ops.keys()])],
  active_operations: (args, { sc, live }) => [
    ...(sc.noDeploying ? [] : (answer("active_operations", args) as unknown[])),
    ...[...live.ops].map(([labId, op]) => ({ labId, op, machine: null, step: null })),
  ],
  lab_launch: launch,
  lab_stop: stop,
  lab_park: park,
  lab_resume: resume,
  lab_provision: provision,
};
