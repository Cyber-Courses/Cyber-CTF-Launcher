import { isLocalVmForm, type RunTarget } from "@/features/labs/run-target";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { emulatorReady, vmFormTarget, isCloudLab, isContainerLab, isLocalHypervisor, runsNativelyHere } from "@/features/labs/lab-runtime";
import { keepsRunningPhase, type LabOperation } from "@/features/labs/runs/operations";
import { lifecyclePhase } from "@/features/labs/lab-phase";
import type { LabControl, LabMachine, LabStatus, Provider, Runtime, ServerHost } from "@/lib/tauri";

// What the lab page shows, derived from the lab's status and what is in flight on it.

export type { LabOperation };
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

/** What the page can do on the built lab: what the backend says the lifecycle accepts where it
 *  runs (its status's `controls`), when this page has the handler wired (the repair, a resume,
 *  is wired with the resume handler where it is offered). */
function labControls(i: LabStateInput) {
  const offers = (c: LabControl) => !!i.status?.controls.includes(c);
  return {
    canPause: i.parkable && offers("pause"),
    canShutdown: i.parkable && offers("shutdown"),
    canProvision: i.provisionable && offers("provision"),
    canRepair: offers("repair"),
  };
}

/** The page's state as one word, most pressing first (`lifecyclePhase`, an operation on it being
 *  "operation"); else whether a run was cut off. */
function labPhase({
  operation,
  interrupted,
  ...lifecycle
}: {
  operation: LabOperation | null;
  running: boolean;
  starting: boolean;
  parked: LabStatus["parked"];
  interrupted: boolean;
  failed: boolean;
}): LabPhase {
  const phase = lifecyclePhase({ busy: !!operation && !keepsRunningPhase(operation), ...lifecycle });
  if (phase === "busy") return "operation";
  return phase ?? (interrupted ? "interrupted" : "notStarted");
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

export function deriveLabState(i: LabStateInput) {
  const { status, runtime: rt } = i;
  const machines = status?.machines ?? [];
  const native = runsNativelyHere(rt, i.hostArch);
  // A VM lab built for another CPU runs here only through an emulator (QEMU), when one is ready.
  const emulates = emulatorReady(i.readyVms);
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
  const isCloud = isCloudLab(rt);
  const interrupted = !deploying && !running && !parked && leftovers(status, isCloud);
  // Machines that went down while the lab runs. Not while it's starting or stopping: machines
  // come up one after another then (a web server waits for its database), and that's normal.
  // The controller is powered off by design once the lab is built: never "down".
  // Nor while it is parked or down altogether: then every machine is off, by design. A machine
  // another process is acting on ("busy": the attack box's start provisioning it) isn't down.
  const down = deploying || !running ? [] : machines.filter((m) => !m.infra && m.state !== "running" && m.state !== "busy");
  const isDocker = isContainerLab(rt);
  const remote = !!status?.host;
  // Where it actually ran, as the backend reports it (its marker survives a failed start, so this
  // is right even for a launch that failed): a dual-form lab is DOCKER-kind but can run as a VM
  // here, and "container" is the only place whose host ports can be swapped for random ones.
  const place = status?.place ?? null;
  const { canPause, canShutdown, canProvision, canRepair } = labControls(i);

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
    place,
    canPause,
    canShutdown,
    canProvision,
    canRepair,
    phase,
  };
}

export type LabState = ReturnType<typeof deriveLabState>;

/** Each published container port and the local port it is bound to, so the player sees the
 *  link between a service inside the lab (web :3206) and the port on this machine (:56235). The
 *  backend lists each declared port once. */
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

/** Where Reset starts the lab again: where it ran, as its status says. On the saved server or
 *  cloud account it ran on (found by id), in the form it ran in there; in a VM here, on the
 *  hypervisor the status names (else, for a container lab, the pick's, else the default), as one
 *  VM per machine or one helper VM for a container lab, as its own VMs for a VM lab; else here. A
 *  host no longer saved can't be started on again: the current pick stands. */
export function restartTarget(
  status: LabStatus | undefined,
  hosts: ServerHost[],
  runtime: Runtime | undefined,
  runOn: RunTarget,
  localVm: Provider | null,
): RunTarget {
  if (status?.hostId) {
    const host = hosts.find((h) => h.id === status.hostId);
    if (!host) return runOn;
    return status.perMachine ? { kind: "host", id: host.id, perMachine: true } : { kind: "host", id: host.id };
  }
  if (status?.place !== "local_vm") return { kind: "local" };
  // A VM lab's own VMs: on the hypervisor they ran on, else the backend's for it.
  if (runtime === "VM") return isLocalHypervisor(status.provider) ? { kind: "local", provider: status.provider } : { kind: "local" };
  const picked = isLocalVmForm(runOn) ? runOn.provider : localVm;
  const provider = isLocalHypervisor(status.provider) ? status.provider : picked;
  if (!provider) return { kind: "local" };
  return vmFormTarget(status.perMachine ? "local-vms" : "local-vm", provider);
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
