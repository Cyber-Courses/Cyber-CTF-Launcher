// The core's run service as the mocks (the browser preview, the component tests) play it: an
// operation's command answers its id at once, and what the mock's handler streams on the command's
// `logs` argument, and how it ends, become the run events the webview follows (`run-events`), as
// the core sends them. A handler that throws before it awaits anything is the core refusing the
// command (the invoke rejects, nothing runs); one that fails later is the operation failing.
import type { CommandError, Cursored, Operation, OperationId, Runtime } from "@/lib/tauri/generated";
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

/** Forgets every event: a new core, which the webview's store follows from scratch. */
export function resetRunService() {
  cursor = 0;
  kept = [];
  forgetRunEvents();
}
registerReset(() => {
  cursor = 0;
  kept = [];
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
 *  module): its id, or the handler's refusal thrown. */
export function startOperation(cmd: string, args: Args, handler: (args: Args) => unknown): OperationId {
  const operation = `op-mock-${String(cursor + 1)}`;
  const labId = labOf(cmd, args);
  const op = OPERATIONS[cmd](args);
  const ended = (error: CommandError | null) => {
    emit({ kind: "ended", operation, labId, op, error });
  };
  // Lines streamed before the operation is told as started (the handler's first, synchronous
  // ones) wait for it.
  let held: string[] | null = [];
  const logs = {
    onmessage: (line: unknown) => {
      if (held) held.push(String(line));
      else emit({ kind: "log", operation, labId, line: String(line) });
    },
  };
  // Run first: a refusal throws here, before the operation exists.
  const running = handler({ ...args, logs });
  const runtime = (typeof args?.runtime === "string" ? args.runtime : null) as Runtime | null;
  const deploying = op !== "stop" && op !== "pause" && op !== "shutdown";
  emit({ kind: "started", operation, labId, op, machine: (args?.machine as string | null) ?? null, runtime, deploying, at: Date.now() });
  for (const line of held) emit({ kind: "log", operation, labId, line });
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
 * The events after `after` (all for none), as `run_events` answers: the ones kept, after the
 * operations `inFlight` says were already running when the core started (the mock's sample of
 * deploys under way), each told as started.
 */
export function replay(after: number | null, inFlight: readonly ActiveOperation[]): Cursored[] {
  if (cursor === 0) {
    for (const o of inFlight) {
      const operation = `op-mock-${String(cursor + 1)}`;
      const frame: Cursored = {
        cursor: ++cursor,
        event: { kind: "started", operation, labId: o.labId, op: o.op, machine: o.machine, runtime: o.runtime ?? null, deploying: o.deploying, at: Date.now() },
      };
      kept.push(frame);
      if (o.step) kept.push({ cursor: ++cursor, event: { kind: "log", operation, labId: o.labId, line: o.step } });
    }
  }
  return kept.filter((f) => after === null || f.cursor > after);
}

/** Ends every operation in flight on `labId` as the core would when it finishes (a mock deploy
 *  running in the background, done), with `error` when it failed. */
export function endOperationsOf(labId: string, error: CommandError | null = null) {
  const open = new Map<string, Operation>();
  for (const { event } of kept) {
    if (event.labId !== labId) continue;
    if (event.kind === "started") open.set(event.operation, event.op);
    if (event.kind === "ended") open.delete(event.operation);
  }
  for (const [operation, op] of open) emit({ kind: "ended", operation, labId, op, error });
}
