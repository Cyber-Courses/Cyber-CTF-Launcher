import type { RunTarget } from "@/features/labs/run-on";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { EMULATORS, runsNatively } from "@/features/labs/lab-runtime";
import { PROVIDER_LABELS } from "@/features/machine/hypervisors";
import { OPERATION_STATUS } from "@/features/labs/runs/operations";
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
  /** What this session's run on the lab is, from the click that began it (known before the
   *  backend's next poll: a clean-up read as "Starting…" until then). */
  runOp?: LabOperation;
  /** The action this page clicked, while it runs. */
  acting: LabAction | null;
  /** The page can pause / shut down, and re-run provisioning (their handlers are wired). */
  parkable: boolean;
  provisionable: boolean;
  /** The last run on this lab was a start that failed (and nothing runs on it now). */
  startFailed?: boolean;
};

/** The page's state as one word (see the status strip). */
export type LabPhase = "operation" | "running" | "starting" | "failed" | "paused" | "shutDown" | "interrupted" | "notStarted";

/** What the page can do on the built lab. A container lab in a VM can only be paused (its
 *  containers don't restart on their own after a power-off); a container lab here only shut down
 *  (its containers stop; there's no state to save). A VM lab gets both. Nothing on a server host
 *  yet. Provisioning runs again in place on VM labs driven by Vagrant (here, or an ESXi host). */
function labControls(i: LabStateInput, { isDocker, isCloud, remote }: { isDocker: boolean; isCloud: boolean; remote: boolean }) {
  const { status } = i;
  return {
    // A cloud lab can't be paused (the backend refuses).
    canPause: i.parkable && !remote && !isCloud && (!isDocker || status?.place === "local_vm"),
    // Not a container lab in a VM (its containers don't come back after a power-off). A VM lab on
    // this machine is reported in a local VM too, and shuts down like any VM lab.
    canShutdown: i.parkable && !remote && !isCloud && !(isDocker && status?.place === "local_vm"),
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
  failed,
}: {
  operation: LabOperation | null;
  running: boolean;
  starting: boolean;
  parked: LabStatus["parked"];
  interrupted: boolean;
  failed: boolean;
}): LabPhase {
  if (operation && OPERATION_STATUS[operation]) return "operation";
  if (running) return "running";
  if (starting) return "starting";
  // Its start just failed: say so, not "Not started" (or "Interrupted", for what it left).
  if (failed) return "failed";
  if (parked) return parked === "pause" ? "paused" : "shutDown";
  return interrupted ? "interrupted" : "notStarted";
}

/** What is in flight on a lab being deployed: the button this page clicked, else what the backend
 *  reports (a reload loses the former), else a stop when a built lab is being worked on, a launch
 *  if not. */
function operationInFlight(i: LabStateInput, running: boolean): LabOperation {
  const ours = i.busy ? i.runOp : undefined;
  return i.acting ?? i.backendOp ?? ours ?? (i.busy && (running || i.status?.parked) ? "stop" : "launch");
}

/** Whether a lab that isn't up left something behind. Only machines that actually exist count: a
 *  VM lab's status lists every declared machine, including ones Vagrant reports as `not_created`,
 *  nothing to clean up (counting them showed "Stop & clean up" on a lab never started). Cloud
 *  services have no machines to count, but the account still recorded (a start that failed or was
 *  cut off) means services may be left behind there, billing. */
function leftovers(status: LabStatus | undefined, isCloud: boolean): boolean {
  if (isCloud) return !!status?.provider;
  return (status?.machines ?? []).some((m) => m.state !== "not_created");
}

/** The engine or hypervisor the status says the lab runs on, by name. */
function engineName(status: LabStatus | undefined): string | null {
  if (!status?.provider) return null;
  return status.provider === "docker" ? "Docker" : (PROVIDER_LABELS[status.provider] ?? status.provider);
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
  const operation = deploying ? operationInFlight(i, running) : null;
  const tearingDown = operation === "stop" || operation === "shutdown" || operation === "pause";
  // Infrastructure exists but nothing is deploying and the lab isn't fully up: a run was cut off
  // (a crash or a restart mid-start). Offer to clean it up rather than a Start that would collide.
  const isCloud = rt?.runtime === "CLOUD";
  const interrupted = !deploying && !running && !parked && leftovers(status, isCloud);
  // Machines that went down while the lab runs. Not while it's starting or stopping: machines
  // come up one after another then (a web server waits for its database), and that's normal.
  // The controller is powered off by design once the lab is built: never "down".
  // Nor while it is parked or down altogether: then every machine is off, by design.
  const down = deploying || !running ? [] : machines.filter((m) => !m.infra && m.state !== "running");
  const isDocker = rt?.runtime !== "VM" && !isCloud;
  const remote = !!status?.host;
  const { canPause, canShutdown, canProvision } = labControls(i, { isDocker, isCloud, remote });
  // Where it actually runs: "on this machine" alone is misleading for a VM lab (which hypervisor
  // do I open?), so name the engine or hypervisor the status reports.
  const engine = engineName(status);

  const phase = labPhase({ operation, running, starting, parked, interrupted, failed: !!i.startFailed && !deploying });

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
  // Docker reports a port once per address family (0.0.0.0 and ::): one row each.
  const seen = new Set<string>();
  return machines.flatMap((m) =>
    m.ports
      .filter((p) => p.published > 0)
      .map((p) => ({ machine: m.name, target: p.target, published: p.published }))
      .filter((b) => {
        const k = `${b.machine}:${String(b.target)}:${String(b.published)}`;
        return !seen.has(k) && !!seen.add(k);
      }),
  );
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
export type HeaderMode = "running" | "parked" | "stopping" | "starting" | "interrupted" | "signIn" | "failed" | "start";

export function headerMode(s: LabState, { loggedIn, canLogin, canResume }: { loggedIn: boolean; canLogin: boolean; canResume: boolean }): HeaderMode {
  if (s.running) return "running";
  if (s.parked && canResume) return "parked";
  // Cleaning up an interrupted run: not running, but this is a stop, not a start.
  if (s.starting) return s.operation === "stop" ? "stopping" : "starting";
  if (s.interrupted) return "interrupted";
  if (!loggedIn && canLogin) return "signIn";
  // Its start failed: Retry (and, for a port clash, Retry on random ports).
  if (s.phase === "failed") return "failed";
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
  // Being stopped (or paused, shut down): the diagram stays, dimmed (see `removing`), so the page
  // doesn't jump, and is still there should the stop fail.
  if (hasMachines && s.tearingDown) return "diagram";
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
  // A running lab keeps it under its panels through a stop, pause or shut down too: moved to the
  // top for the run and back after it, the page jumped twice. How it went is said under the
  // header (a failure notice) and in a toast.
  return s.running && (!busy || s.tearingDown) ? "bottom" : "top";
}

/** The lab is being stopped and removed: its panels stay, dimmed and inert, until it is gone. */
export const removing = (s: LabState) => s.deploying && s.operation === "stop";

/** The window width from which a lab's diagram can leave the main column for the page's full
 *  width (Tailwind's 2xl): narrower, the rail beside it would leave the diagram too little. */
export const WIDE_LAB_PAGE = "(min-width: 96rem)";

/** Whether the network diagram spans the page above the main column and the side rail: on a
 *  wide window, once it is drawn, and not while the deployment's steps lead the page (those stay
 *  first, the diagram after them, as on a narrow window). */
export function diagramAbove(wide: boolean, view: ReturnType<typeof networkView>, placement: ReturnType<typeof deployPlacement>) {
  return wide && view === "diagram" && placement !== "top";
}
