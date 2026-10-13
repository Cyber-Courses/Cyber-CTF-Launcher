import type { RunTarget } from "@/features/labs/run-on";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { EMULATORS, runsNatively } from "@/features/labs/lab-runtime";
import { PROVIDER_LABELS } from "@/features/machine/hypervisors";
import { OPERATION_STATUS } from "@/lib/deploy-store";
import type { ActiveOperation, LabMachine, LabStatus, Provider, ServerHost } from "@/lib/tauri";

// What the lab page shows, derived from the lab's status and what is in flight on it.

export type LabOperation = ActiveOperation["op"];
/** A header or panel action this page runs on a built lab. */
export type LabAction = "pause" | "shutdown" | "resume" | "provision";

export type LabStateInput = {
  status: LabStatus | undefined;
  runtime: LabRuntimeInfo | null;
  hostArch: string;
  /** Local hypervisors ready on this machine. */
  readyVms: Provider[];
  /** A run this session started is in flight. */
  busy: boolean;
  /** The backend reports a deploy on this lab (it survives a window reload; `busy` doesn't). */
  backendDeploying: boolean;
  /** What the backend reports in flight on this lab. */
  backendOp: LabOperation | undefined;
  /** The action this page clicked, while it runs. */
  acting: LabAction | null;
  /** The page can pause / shut down, and re-run provisioning (their handlers are wired). */
  parkable: boolean;
  provisionable: boolean;
};

/** The page's state as one word (see the status strip). */
export type LabPhase = "operation" | "running" | "starting" | "paused" | "shutDown" | "interrupted" | "notStarted";

/** What the page can do on the built lab. A container lab in a VM can only be paused (its
 *  containers don't restart on their own after a power-off); a container lab here only shut down
 *  (its containers stop; there's no state to save). A VM lab gets both. Nothing on a server host
 *  yet. Provisioning runs again in place on VM labs driven by Vagrant (here, or an ESXi host). */
function labControls(i: LabStateInput, { isDocker, isCloud, remote }: { isDocker: boolean; isCloud: boolean; remote: boolean }) {
  const { status } = i;
  return {
    canPause: i.parkable && !remote && (!isDocker || status?.place === "local_vm"),
    canShutdown: i.parkable && !remote && !isCloud && status?.place !== "local_vm",
    canProvision: i.provisionable && !isDocker && !isCloud && status?.place !== "cloud" && status?.provider !== "proxmox",
  };
}

/** The page's state as one word, most pressing first. */
function labPhase({
  operation,
  running,
  starting,
  parked,
  interrupted,
}: {
  operation: LabOperation | null;
  running: boolean;
  starting: boolean;
  parked: LabStatus["parked"];
  interrupted: boolean;
}): LabPhase {
  if (operation && OPERATION_STATUS[operation]) return "operation";
  if (running) return "running";
  if (starting) return "starting";
  if (parked) return parked === "pause" ? "paused" : "shutDown";
  return interrupted ? "interrupted" : "notStarted";
}

export function deriveLabState(i: LabStateInput) {
  const { status, runtime: rt } = i;
  const machines = status?.machines ?? [];
  // Unknown host (report not in yet) or a lab for any CPU: native, never a wrong "emulated".
  const native = !i.hostArch || !rt || runsNatively(rt, i.hostArch);
  // A VM lab built for another CPU runs here only through an emulator (QEMU), when one is ready.
  const emulates = i.readyVms.some((p) => EMULATORS.includes(p));
  const running = status?.running ?? false;
  // A deploy this session started sets `busy`; one still running after a window reload (which
  // loses the in-memory deploy state) is recovered from the backend, so the page shows "Starting"
  // instead of a bare Start button that would invite a colliding second start.
  const deploying = i.busy || i.backendDeploying;
  const starting = deploying && !running;
  // Parked by the launcher: its stopped machines are expected, and come back on Resume.
  const parked = !running && !deploying ? (status?.parked ?? null) : null;
  // What is in flight on this lab: the button this page clicked, else what the backend reports
  // (a reload loses the former), else a stop when a built lab is being worked on, a launch if not.
  const operation: LabOperation | null = deploying ? (i.acting ?? i.backendOp ?? (i.busy && (running || status?.parked) ? "stop" : "launch")) : null;
  const tearingDown = operation === "stop" || operation === "shutdown" || operation === "pause";
  // Infrastructure exists but nothing is deploying and the lab isn't fully up: a run was cut off
  // (a crash or a restart mid-start). Offer to clean it up rather than a Start that would collide.
  // Only machines that actually exist count as leftovers: a VM lab's status lists every declared
  // machine, including ones Vagrant reports as `not_created`, and those are nothing to clean up
  // (counting them showed "Stop & clean up" on a lab that was never started).
  // Cloud services: no machines to count, but the account still recorded (a start that failed or
  // was cut off) means services may be left behind there, billing.
  const isCloud = rt?.runtime === "CLOUD";
  const leftovers = isCloud ? !!status?.provider : machines.some((m) => m.state !== "not_created");
  const interrupted = !deploying && !running && !parked && leftovers;
  // Machines that went down while the lab runs. Not while it's starting or stopping: machines
  // come up one after another then (a web server waits for its database), and that's normal.
  // The controller is powered off by design once the lab is built: never "down".
  const down = deploying ? [] : machines.filter((m) => !m.infra && m.state !== "running");
  const isDocker = rt?.runtime !== "VM" && !isCloud;
  const remote = !!status?.host;
  const { canPause, canShutdown, canProvision } = labControls(i, { isDocker, isCloud, remote });
  // Where it actually runs: "on this machine" alone is misleading for a VM lab (which hypervisor
  // do I open?), so name the engine or hypervisor the status reports.
  const engine = status?.provider ? (status.provider === "docker" ? "Docker" : (PROVIDER_LABELS[status.provider] ?? status.provider)) : null;

  const phase = labPhase({ operation, running, starting, parked, interrupted });

  return {
    native,
    emulates,
    running,
    deploying,
    starting,
    parked,
    operation,
    tearingDown,
    interrupted,
    down,
    isDocker,
    isCloud,
    remote,
    canPause,
    canShutdown,
    canProvision,
    engine,
    phase,
  };
}

export type LabState = ReturnType<typeof deriveLabState>;

/** Each published container port and the local port it is bound to, so the player sees the
 *  link between a service inside the lab (web :3206) and the port on this machine (:56235). */
export function publishedBinds(machines: LabMachine[]) {
  return machines.flatMap((m) => m.ports.filter((p) => p.published > 0).map((p) => ({ machine: m.name, target: p.target, published: p.published })));
}

/** The host a lab's published ports answer on: its URL's, else loopback. */
export function bindHost(url: string | null | undefined): string {
  if (!url) return "127.0.0.1";
  try {
    return new URL(url).hostname;
  } catch {
    return "127.0.0.1";
  }
}

/** Where Reset starts the lab again: the saved host it runs on; a VM on this machine when its
 *  status names a host that isn't a saved one; else here. */
export function restartTarget(status: LabStatus | undefined, hosts: ServerHost[], runOn: RunTarget, localVm: Provider | null): RunTarget {
  const host = hosts.find((h) => h.name === status?.host);
  if (host) return { kind: "host", id: host.id };
  const vm = status?.host ? (runOn.kind === "local-vm" ? runOn.provider : localVm) : null;
  return vm ? { kind: "local-vm", provider: vm } : { kind: "local" };
}

/** The header's actions, by what the lab is doing. */
export type HeaderMode = "running" | "parked" | "stopping" | "starting" | "interrupted" | "signIn" | "start";

export function headerMode(s: LabState, { loggedIn, canLogin, canResume }: { loggedIn: boolean; canLogin: boolean; canResume: boolean }): HeaderMode {
  if (s.running) return "running";
  if (s.parked && canResume) return "parked";
  // Cleaning up an interrupted run: not running, but this is a stop, not a start.
  if (s.starting) return s.operation === "stop" ? "stopping" : "starting";
  if (s.interrupted) return "interrupted";
  if (!loggedIn && canLogin) return "signIn";
  return "start";
}

/** The note the network panel shows in place of the diagram (`labs.detail.<note>`). */
export type NetworkNote = "interruptedProvision" | "interruptedClean" | "pausedNote" | "shutDownNote" | "readingRemote" | "startToSee";

/** The network panel: the diagram once the lab is up (or parked, or an action of this page runs
 *  on it), nothing while it deploys, else a note saying why there is no diagram. */
export function networkView(
  s: LabState,
  { busy, acting, hasMachines }: { busy: boolean; acting: LabAction | null; hasMachines: boolean },
): "diagram" | "none" | NetworkNote {
  // Only once the lab is up and no longer deploying: during a build the backend may already
  // report a machine "running" while it's still being provisioned, and a half-resolved diagram is
  // more confusing than helpful. Pausing, resuming or provisioning a lab, and a parked lab, keep
  // the diagram: its machines exist and their states (paused, off, running) are the point.
  if (hasMachines && (acting !== null || (!busy && (s.running || s.parked)))) return "diagram";
  if (busy || s.starting) return "none";
  if (s.interrupted) return s.canProvision ? "interruptedProvision" : "interruptedClean";
  if (s.parked) return s.parked === "pause" ? "pausedNote" : "shutDownNote";
  if (s.running && s.remote) return "readingRemote";
  return "startToSee";
}

/** A cloud lab's main panel, in place of the network: its services once up, nothing while it
 *  deploys, else a note (`labs.detail.<note>`): services left behind, or what a start does. */
export function cloudView(s: LabState, { busy }: { busy: boolean }): "services" | "none" | "cloudLeftover" | "cloudIdle" {
  if (s.running && !busy) return "services";
  if (busy || s.starting) return "none";
  return s.interrupted ? "cloudLeftover" : "cloudIdle";
}

/** Where the deployment panel goes: worth showing while a run is in progress, once the lab is
 *  up, or when the last run failed (once a lab is stopped the leftover "✓ Lab is running" logs
 *  are stale). While it starts it comes first; once ready, the diagram does. */
export function deployPlacement(s: LabState, { busy, failed }: { busy: boolean; failed: boolean }): "top" | "bottom" | null {
  if (!(s.deploying || s.running || failed)) return null;
  return s.running && !busy ? "bottom" : "top";
}
