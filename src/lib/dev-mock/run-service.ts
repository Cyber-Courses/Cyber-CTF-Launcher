// The core's run service as the mocks (the browser preview, the component tests) play it: an
// operation's command answers its id at once, and what the mock's handler streams on the command's
// `logs` argument, and how it ends, become the run events the webview follows (`run-events`), as
// the core sends them. A handler that throws before it awaits anything is the core refusing the
// command (the invoke rejects, nothing runs); one that fails later is the operation failing.
import type { CommandError, Cursored, DeployStep, LabStatus, Operation, OperationId, Runtime } from "@/lib/tauri/generated";

type RunEvent = Cursored["event"];
import type { ActiveOperation } from "@/lib/tauri/run-events";
import { forgetRunEvents, takeIn } from "@/lib/tauri/run-events";
import { registerReset } from "@/lib/module-state";
import type { Args } from "@/lib/dev-mock/types";
import { argText } from "@/lib/dev-mock/command";

/** The commands that start a lab operation, and the operation each is. */
const OPERATIONS: Record<string, (args: Args) => Operation> = {
  lab_launch: () => "launch",
  lab_launch_local: () => "launch",
  lab_stop: () => "stop",
  lab_park: (args) => (args?.mode === "shutdown" ? "shutdown" : "pause"),
  lab_resume: () => "resume",
  lab_provision: () => "provision",
  attack_box_attach: () => "attach",
  attack_box_detach: () => "detach",
};

/** Whether `cmd` starts a lab operation. */
export const isOperation = (cmd: string) => cmd in OPERATIONS;

let cursor = 0;
let kept: Cursored[] = [];
/** Whether the core's start was told (`seed`): what it was running, what its first scan saw up. */
let seeded = false;

/** Forgets every event: a new core, which the webview's store follows from scratch. */
export function resetRunService() {
  cursor = 0;
  kept = [];
  seeded = false;
  forgetRunEvents();
}
registerReset(() => {
  cursor = 0;
  kept = [];
  seeded = false;
});

/** Emits `event` as the core does: kept for a window that (re)loads, and taken in at once. */
function emit(event: Cursored["event"]) {
  const frame = { cursor: ++cursor, event };
  kept.push(frame);
  takeIn([frame]);
}

/** The lab an operation command acts on. */
const labOf = (cmd: string, args: Args): string => {
  if (cmd === "lab_launch") return argText(args?.labId);
  if (cmd === "lab_launch_local") return `local-${argText(args?.path).split(/[\\/]/).pop() ?? "lab"}`;
  return argText(args?.id);
};

/** Runs operation command `cmd` with `handler` (its mock), as the core's run service does (see the
 *  module): its id, or the handler's refusal thrown. Once it ended, what `scan` (the mock's
 *  `running_labs`) sees up is told, as the core's reconcile tells the scan an operation changed. */
export function startOperation(cmd: string, args: Args, handler: (args: Args) => unknown, scan?: () => Promise<string[]>): OperationId {
  const operation = `op-mock-${String(cursor + 1)}`;
  const labId = labOf(cmd, args);
  const op = OPERATIONS[cmd](args);
  const ended = (error: CommandError | null) => {
    emit({ kind: "ended", operation, labId, op, error });
    scan?.().then(tellRunning, () => undefined);
  };
  // What the handler streams: a log line, or a step the runner says (a `DeployStep`). What it
  // streams before the operation is told as started (its first, synchronous ones) waits for it.
  const told = (said: unknown): RunEvent =>
    typeof said === "object" && said !== null && "step" in said
      ? { kind: "step", operation, labId, step: said as DeployStep }
      : { kind: "log", operation, labId, line: String(said) };
  let held: RunEvent[] | null = [];
  const logs = {
    onmessage: (said: unknown) => {
      if (held) held.push(told(said));
      else emit(told(said));
    },
  };
  // Run first: a refusal throws here, before the operation exists.
  const running = handler({ ...args, logs });
  const runtime = (typeof args?.runtime === "string" ? args.runtime : null) as Runtime | null;
  const deploying = op !== "stop" && op !== "pause" && op !== "shutdown";
  emit({ kind: "started", operation, labId, op, machine: (args?.machine as string | null) ?? null, runtime, deploying, at: Date.now() });
  for (const event of held) emit(event);
  held = null;
  Promise.resolve(running).then(
    () => {
      ended(null);
    },
    (e: unknown) => {
      ended(asError(e));
    },
  );
  return operation;
}

/** `e` as the core sends an error. */
function asError(e: unknown): CommandError {
  if (e && typeof e === "object" && "code" in e && "message" in e) return e as CommandError;
  return { code: "other", message: e instanceof Error ? e.message : String(e) };
}

/**
 * Tells, once, what the core was doing when it started, as its first events: what its first scan
 * saw up (`scan`), then each operation `inFlight` says was already running (the mock's sample of
 * deploys under way), told as started, at its step. Whichever reaches the core first seeds it: a
 * window's replay, its watch of the labs, or an operation. Seeded by the replay alone, the sample's
 * deploys were lost whenever the watch told the scan first: the window's cursor was past them.
 */
export function seed(inFlight: readonly ActiveOperation[], scan: readonly string[] = []) {
  if (seeded) return;
  seeded = true;
  emit({ kind: "running", labIds: [...scan] });
  for (const o of inFlight) {
    const operation = `op-mock-${String(cursor + 1)}`;
    emit({ kind: "started", operation, labId: o.labId, op: o.op, machine: o.machine, runtime: o.runtime ?? null, deploying: o.deploying, at: Date.now() });
    if (o.step) emit({ kind: "log", operation, labId: o.labId, line: o.step });
  }
}

/**
 * The events after `after` (all for none), as `run_events` answers: the ones kept, the core's
 * start first (`seed`, with `inFlight` and `scan`, when nothing seeded it yet).
 */
export function replay(after: number | null, inFlight: readonly ActiveOperation[], scan: readonly string[] = []): Cursored[] {
  seed(inFlight, scan);
  return kept.filter((f) => after === null || f.cursor > after);
}

/** Ends every operation in flight on `labId` as the core would when it finishes (a mock deploy
 *  running in the background, done), with `error` when it failed. */
export function endOperationsOf(labId: string, error: CommandError | null = null) {
  const open = new Map<string, Operation>();
  for (const { event } of kept) {
    if ((event.kind !== "started" && event.kind !== "ended") || event.labId !== labId) continue;
    if (event.kind === "started") open.set(event.operation, event.op);
    if (event.kind === "ended") open.delete(event.operation);
  }
  for (const [operation, op] of open) emit({ kind: "ended", operation, labId, op, error });
}

/**
 * `labs_watch` as the core's reconcile answers it: the scan's labs up (the mock's `running_labs`)
 * and each watched lab's status (its `lab_status`, or why that failed), told as lab events. The
 * mock tells them at once, on every watch: a test that changes a status watches again, or tells
 * it with `tellStatus`.
 */
export async function watchLabs(args: Args, handle: (cmd: string, args: Args) => unknown): Promise<null> {
  const labs = (args?.labs as { id: string; runtime: Runtime }[] | undefined) ?? [];
  const running = (await handle("running_labs", undefined)) as string[] | null;
  emit({ kind: "running", labIds: running ?? [] });
  for (const { id, runtime } of labs) {
    try {
      tellStatus(id, (await handle("lab_status", { id, runtime })) as LabStatus);
    } catch (e: unknown) {
      emit({ kind: "status", labId: id, status: null, error: asError(e) });
    }
  }
  return null;
}

/** Tells lab `labId`'s status as the core's reconcile does when it changed. */
export function tellStatus(labId: string, status: LabStatus) {
  emit({ kind: "status", labId, status, error: null });
}

/** Tells what the core's scan sees up, as its reconcile does when that changed. */
export function tellRunning(labIds: string[]) {
  emit({ kind: "running", labIds });
}
