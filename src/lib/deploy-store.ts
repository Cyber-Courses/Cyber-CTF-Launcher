"use client";

import { useState, useSyncExternalStore } from "react";
import { activeOperations, deployingLabs, labDeployLog, type ActiveOperation } from "@/lib/tauri";
import { ignore } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";
import { sharedPoll, useSharedPoll } from "@/lib/shared-poll";
import { translate } from "@/lib/i18n";

/**
 * The live state of lab start/stop, kept outside the React tree so it survives navigating
 * away from the labs screen (which unmounts it) while a deploy keeps running in the backend.
 * `useLabActions` writes here; any screen reads the same snapshot and sees the in-progress
 * logs and which lab is busy when it mounts.
 */
/** One lab's live run. Kept per lab so two deploys at once don't share a log stream. */
export type DeployRun = {
  /** Whether this lab is currently starting or stopping. */
  busy: boolean;
  /** The streamed log lines of this lab's current run. */
  logs: string[];
  /** The arrival time (ms) of each log line, parallel to `logs`, so step timings survive
   *  leaving and returning to the lab page. */
  times: number[];
  /** What the run is (launch, stop, shutdown…), so a list row can say "Starting…" or
   *  "Stopping…" instead of guessing from the lab's status. */
  op?: string;
};
/** Every lab with a run in flight or just finished, keyed by lab id. A single global slot
 *  would cross-contaminate: with two deploys running, the second's lines would appear on the
 *  first's page. Keying by lab id keeps each lab's log stream its own. */
export type DeployState = { runs: Record<string, DeployRun> };

const EMPTY_RUN: DeployRun = { busy: false, logs: [], times: [] };

let state: DeployState = { runs: {} };
const listeners = new Set<() => void>();

function emit() {
  for (const l of listeners) l();
}

function setRun(labId: string, run: DeployRun) {
  state = { runs: { ...state.runs, [labId]: run } };
  emit();
}

// Log lines waiting to be added, by lab: a deploy streams hundreds of lines a second at times,
// and copying a run's logs and re-rendering every reader per line was most of a busy page's work.
// They are added together about every 100 ms (a timer, not an animation frame: frames stop in a
// hidden window, and a deploy finishing in the background must still end with all its lines).
const pending = new Map<string, { lines: string[]; times: number[] }>();
let flushTimer: ReturnType<typeof setTimeout> | null = null;
const FLUSH_MS = 100;

/** Adds the waiting log lines now (one update for all of them). */
export function flushDeployLogs() {
  if (flushTimer !== null) clearTimeout(flushTimer);
  flushTimer = null;
  if (pending.size === 0) return;
  const runs = { ...state.runs };
  for (const [labId, { lines, times }] of pending) {
    const prev = runs[labId] ?? EMPTY_RUN;
    runs[labId] = { ...prev, busy: true, logs: [...prev.logs, ...lines], times: [...prev.times, ...times] };
  }
  pending.clear();
  state = { runs };
  emit();
}

/** Begin a run (`op`) for `labId`: clears that lab's previous logs, leaves other labs untouched. */
export function beginDeploy(labId: string, op?: string) {
  flushDeployLogs();
  setRun(labId, { busy: true, logs: [], times: [], op });
}

/** What a lab shows while an operation runs on it: the containers report "running" well before
 *  a launch or resume is done, and until the end of a shut down or stop. */
export const OPERATION_STATUS: Partial<Record<string, string>> = {
  // Getters: read in the current language each time.
  get launch() {
    return translate("common.operation.status.launch");
  },
  get resume() {
    return translate("common.operation.status.resume");
  },
  get pause() {
    return translate("common.operation.status.pause");
  },
  get shutdown() {
    return translate("common.operation.status.shutdown");
  },
  get stop() {
    return translate("common.operation.status.stop");
  },
};

/** Append a log line to `labId`'s run, timestamped on arrival (shown with the next batch). */
export function appendDeployLog(labId: string, line: string) {
  noteSignedOut(line);
  const batch = pending.get(labId) ?? { lines: [], times: [] };
  batch.lines.push(line);
  batch.times.push(Date.now());
  pending.set(labId, batch);
  if (flushTimer === null) flushTimer = setTimeout(flushDeployLogs, FLUSH_MS);
}

/** Fired when an action failed because the session is gone; the shell re-reads the sign-in. */
export const SIGNED_OUT_EVENT = "cyberctf:signed-out";

/** The backend's "You're signed out…" / "…session expired…" errors (account/auth.rs). */
function noteSignedOut(text: string) {
  if (typeof window !== "undefined" && /\bsigned out\b/i.test(text)) window.dispatchEvent(new Event(SIGNED_OUT_EVENT));
}

/** End `labId`'s run (keeps its logs so the console stays readable). */
export function endDeploy(labId: string) {
  flushDeployLogs();
  const prev = state.runs[labId];
  if (!prev) return;
  setRun(labId, { ...prev, busy: false });
}

/** Put `labId`'s run back as it was before a `beginDeploy` that turned out not to start
 *  anything (the backend refused it), so the previous run's log isn't replaced by a failure. */
export function restoreDeploy(labId: string, run: DeployRun | undefined) {
  flushDeployLogs();
  if (run) return setRun(labId, run);
  if (!(labId in state.runs)) return;
  const rest = { ...state.runs };
  delete rest[labId];
  state = { runs: rest };
  emit();
}

/** Subscribe to store changes; returns an unsubscribe. Exposed so the store can be driven and
 *  asserted in tests without rendering React. */
export function subscribeDeploy(cb: () => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** The current state outside React (an operation's end, tests), with every line in so far. */
export function getDeploySnapshot(): DeployState {
  flushDeployLogs();
  return state;
}

const snapshot = () => state;

export function useDeploy(): DeployState {
  return useSyncExternalStore(subscribeDeploy, snapshot, snapshot);
}

/**
 * The lab ids the backend is starting or stopping right now, polled from the long-lived Rust
 * process. The in-memory store above is lost when the window reloads (a dev rebuild, a crash, or
 * fast-refresh), but the deploy keeps running; reading this lets a screen show "this lab is
 * starting" again instead of a bare Start button (which would invite a colliding second start).
 */
export function useDeployingLabs(): Set<string> {
  return useSharedPoll(deployingPoll);
}

// One poll of each for the whole window, whichever screens read it (see shared-poll).
const deployingPoll = sharedPoll({ read: () => deployingLabs().then((l) => new Set(l)), ms: 4000, initial: new Set<string>(), onFocus: true });

/** The labs deploying in the backend as last read (empty while nothing reads them). */
export const deployingLabsNow = () => deployingPoll.get();

/** The sidebar's label for an operation, e.g. "Pausing…"; the machine when it targets one. */
export function operationLabel(o: ActiveOperation): string {
  switch (o.op) {
    case "launch":
      return translate("common.operation.label.launch");
    case "resume":
      return translate("common.operation.label.resume");
    case "pause":
      return translate("common.operation.label.pause");
    case "shutdown":
      return translate("common.operation.label.shutdown");
    case "provision":
      return o.machine
        ? translate("common.operation.label.provisionMachine", { machine: o.machine.replace(/^isoloom-/, "") })
        : translate("common.operation.label.provision");
    case "attack_vm":
      return translate("common.operation.label.attackVm");
    case "stop":
      return translate("common.operation.label.stop");
  }
}

/**
 * Every operation in flight, by lab, with the step its log is at. Polled often (steps move
 * every few seconds during a VM start), so the sidebar reads like a live progress line.
 */
export function useActiveOperations(): Map<string, ActiveOperation> {
  return useSharedPoll(operationsPoll);
}

const operationsPoll = sharedPoll({
  read: () => activeOperations().then((l) => new Map(l.map((o) => [o.labId, o]))),
  ms: 2000,
  initial: new Map<string, ActiveOperation>(),
  // Read on in a hidden window, slower while nothing runs: the shell starts a lab's attack box
  // when its launch ends (from the website too), which must not wait for the window to show.
  hiddenMs: (ops) => (ops.size > 0 ? 2000 : 10_000),
});

/** The operations in flight as last read (empty while nothing reads them). */
export const activeOperationsNow = () => operationsPoll.get();

/** A log with each line's arrival time, parallel. */
export type TimedLog = { lines: string[]; times: number[] };
const NO_LOG: TimedLog = { lines: [], times: [] };

/** `prev` followed by what a new read of the whole log (`lines`) adds, the new lines timed `at`;
 *  a log that shrank or starts differently is a new run, timed afresh. The same lines again keep
 *  `prev` as is. */
export function timeLines(prev: TimedLog, lines: string[], at: number): TimedLog {
  const restart = lines.length < prev.lines.length || (prev.lines.length > 0 && prev.lines[0] !== lines[0]);
  const base = restart ? NO_LOG : prev;
  if (base.lines.length === lines.length) return base;
  return { lines, times: [...base.times, ...lines.slice(base.lines.length).map(() => at)] };
}

/**
 * The log of a lab's deploy running in its detached worker process, followed while `active`,
 * each line timed when it was first read (for the steps' durations). For a page that
 * (re)attaches to a deploy after a reload or an app relaunch: the in-memory store has no lines
 * of it, the worker's log file has them all.
 */
export function useWorkerLog(labId: string, active: boolean, pollMs = 1500): TimedLog {
  const [log, setLog] = useState<TimedLog>(NO_LOG);
  usePoll(() => labDeployLog(labId).then((text) => text.split("\n").filter((l) => l.length > 0)), pollMs, {
    enabled: active,
    restartKey: labId,
    onError: ignore("read again in a moment"),
    onValue: (lines) => setLog((prev) => timeLines(prev, lines, Date.now())),
  });
  return log;
}
