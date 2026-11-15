import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { EVENTS } from "@/lib/tauri/generated/Events";
import type { CommandError, Cursored, Operation, OperationId, Runtime } from "@/lib/tauri/generated";
import { registerReset } from "@/lib/module-state";

// The one store of the lab operations, fed by the core's run events (`run_service`): each
// operation's start, its log lines and its end. Nothing here polls: a window subscribes to the
// events once, replays the ones it missed (a reload, a relaunch: the core keeps them), then
// follows the live ones. Every view of an operation (the deploy panel, the sidebar's step, the
// labs busy, an action awaiting its end) reads it.

/** One operation in flight on a lab, as the screens read it: what it is, the machine it targets,
 *  whether the lab shows as deploying, the step its log is at and the runtime it acts in. */
export type ActiveOperation = {
  labId: string;
  op: Operation;
  machine: string | null;
  deploying: boolean;
  step: string | null;
  runtime?: Runtime | null;
};

/** One operation, as its events told it so far. */
export type OperationRun = {
  operation: OperationId;
  labId: string;
  op: Operation;
  machine: string | null;
  runtime: Runtime | null;
  deploying: boolean;
  /** When it began (ms since the epoch, the core's clock). */
  startedAt: number;
  /** Its log lines, and when this window received each (ms), parallel. */
  lines: string[];
  times: number[];
  /** Whether it ended; `error` is why it failed (null: it went through, or it runs). */
  ended: boolean;
  error: CommandError | null;
};

export type RunEventsState = {
  /** The last event's cursor taken in. */
  cursor: number;
  /** The operations in flight and each lab's last ended one, by id. */
  operations: ReadonlyMap<OperationId, OperationRun>;
};

const EMPTY: RunEventsState = { cursor: 0, operations: new Map() };
let state: RunEventsState = EMPTY;
const listeners = new Set<() => void>();
let connected: Promise<void> | null = null;

/** Forgets every operation and the subscription: a window with no events yet (the module's
 *  reset, and a mock core starting over). */
export function forgetRunEvents() {
  state = EMPTY;
  connected = null;
  for (const l of listeners) l();
}
registerReset(forgetRunEvents);

/** `run` with the frame's event applied (the operation it is of, as it now stands). */
function applied(runs: Map<OperationId, OperationRun>, { event }: Cursored, at: number, copied: Set<OperationId>) {
  // A lab's state, not an operation's: for the readers of `onLabEvent`.
  if (event.kind === "status" || event.kind === "running") return;
  const run = runs.get(event.operation);
  switch (event.kind) {
    case "started":
      runs.set(event.operation, {
        operation: event.operation,
        labId: event.labId,
        op: event.op,
        machine: event.machine,
        runtime: event.runtime,
        deploying: event.deploying,
        startedAt: event.at,
        lines: [],
        times: [],
        ended: false,
        error: null,
      });
      return;
    case "log":
      if (!run) return;
      // Copied once per batch (readers hold the previous arrays), then added to in place: a VM
      // start's thousands of lines aren't copied once per line.
      if (copied.has(event.operation)) {
        run.lines.push(event.line);
        run.times.push(at);
      } else {
        copied.add(event.operation);
        runs.set(event.operation, { ...run, lines: [...run.lines, event.line], times: [...run.times, at] });
      }
      return;
    case "ended":
      if (!run) return;
      runs.set(event.operation, { ...run, ended: true, error: event.error });
      // A lab keeps its last ended operation only: the one before it is forgotten.
      for (const [id, other] of runs) if (id !== event.operation && other.labId === run.labId && other.ended) runs.delete(id);
  }
}

/** Takes in `frames` (in order; one already taken in is skipped) and tells the readers once. */
export function takeIn(frames: readonly Cursored[]) {
  const fresh = frames.filter((f) => f.cursor > state.cursor);
  if (fresh.length === 0) return;
  const runs = new Map(state.operations);
  const at = Date.now();
  const copied = new Set<OperationId>();
  for (const f of fresh) applied(runs, f, at, copied);
  state = { cursor: fresh.at(-1)?.cursor ?? state.cursor, operations: runs };
  for (const l of listeners) l();
  for (const f of fresh) if (f.event.kind === "status" || f.event.kind === "running") for (const l of labListeners) l(f.event);
}

/** A lab's state as the core's reconcile tells it: a status read, or what the scan sees up. */
export type LabEvent = Extract<Cursored["event"], { kind: "status" | "running" }>;
const labListeners = new Set<(e: LabEvent) => void>();

/** Hears every lab event taken in from now on (the lab store's feed); returns an unsubscribe. */
export function onLabEvent(cb: (e: LabEvent) => void) {
  labListeners.add(cb);
  return () => {
    labListeners.delete(cb);
  };
}

/** Subscribes this window to the run events, once: the live ones are held while the missed ones
 *  are replayed, then taken in after them, so none is lost or taken twice. */
export function connectRunEvents(): Promise<void> {
  connected ??= (async () => {
    let replayed = false;
    const held: Cursored[] = [];
    await listen<Cursored>(EVENTS.runEvent, (e) => {
      if (replayed) takeIn([e.payload]);
      else held.push(e.payload);
    });
    const missed = (await invoke<Cursored[] | null>("run_events", { after: state.cursor || null })) ?? [];
    replayed = true;
    takeIn(missed);
    takeIn(held);
  })();
  return connected;
}

/** The operation a failure is of, when it is an operation's (what a player quotes when asking for
 *  help: the same id is in the app's log and in the runner's journal). */
export function operationOf(e: unknown): OperationId | null {
  if (typeof e !== "object" || e === null || !("operation" in e)) return null;
  return typeof e.operation === "string" ? e.operation : null;
}

/** Subscribes to every change; returns an unsubscribe. */
export function subscribeRunEvents(cb: () => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** The operations as they stand. */
export const runEventsNow = (): RunEventsState => state;

/** The operations in flight, oldest first. */
export const inFlight = (s: RunEventsState = state): OperationRun[] =>
  [...s.operations.values()].filter((r) => !r.ended).sort((a, b) => a.startedAt - b.startedAt);

/** The last operation of `labId` (in flight, else the last one ended), if this window knows one. */
export function lastRunOf(labId: string, s: RunEventsState = state): OperationRun | undefined {
  const runs = [...s.operations.values()].filter((r) => r.labId === labId);
  return runs.find((r) => !r.ended) ?? runs.at(-1);
}

/**
 * Follows operation `operation` to its end: each of its log lines to `onLog`, in order (those
 * already in too), then resolves when it went through or rejects with its error, as the core
 * sent it (a command error: the code and message the rest of the webview reads).
 */
export async function awaitOperation(operation: OperationId, onLog: (line: string) => void): Promise<void> {
  await connectRunEvents();
  return new Promise<void>((resolve, reject) => {
    let sent = 0;
    const check = () => {
      const run = state.operations.get(operation);
      if (!run) return;
      while (sent < run.lines.length) onLog(run.lines[sent++]);
      if (!run.ended) return;
      off();
      // Rejected with an Error that reads as the core's error (its code, message and args), as a
      // failed command's does to the rest of the webview, and names the operation (`operationOf`).
      if (run.error) reject(Object.assign(new Error(run.error.message), run.error, { operation }));
      else resolve();
    };
    const off = subscribeRunEvents(check);
    check();
  });
}
