"use client";

import { useSyncExternalStore } from "react";
import { describeError, errorCode, type ErrorCode } from "@/lib/errors";
import { registerReset } from "@/lib/module-state";

// The live state of lab start/stop, kept outside the React tree so it survives navigating away
// from the labs screen (which unmounts it) while a deploy keeps running in the backend.
// `useLabActions` writes here; any screen reads the same snapshot and sees the in-progress logs
// and which lab is busy when it mounts.

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
  /** When it began (ms): its timers count from here, not from its first log line or from when
   *  a page showing it opened. */
  startedAt?: number;
  /** The code of the error it failed with (its ✗ line is in the player's language). */
  failureCode?: ErrorCode | null;
};
/** Every lab with a run in flight or just finished, keyed by lab id (none for a lab without
 *  one). A single global slot would cross-contaminate: with two deploys running, the second's
 *  lines would appear on the first's page. Keying by lab id keeps each lab's log stream its own. */
export type DeployState = { runs: Partial<Record<string, DeployRun>> };

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

// Forgets every run (see registerReset).
registerReset(() => {
  if (flushTimer !== null) clearTimeout(flushTimer);
  flushTimer = null;
  pending.clear();
  state = { runs: {} };
  emit();
});

/** Adds the waiting log lines now (one update for all of them). */
function flushDeployLogs() {
  if (flushTimer !== null) clearTimeout(flushTimer);
  flushTimer = null;
  if (pending.size === 0) return;
  const runs = { ...state.runs };
  for (const [labId, { lines, times }] of pending) {
    const prev = runs[labId] ?? EMPTY_RUN;
    // `busy` is begin/end's to set: a channel's last line can arrive after the command returned,
    // and turned a finished run back into a "Starting…" that nothing ended.
    runs[labId] = { ...prev, logs: [...prev.logs, ...lines], times: [...prev.times, ...times] };
  }
  pending.clear();
  state = { runs };
  emit();
}

/** Begin a run (`op`) for `labId`: clears that lab's previous logs, leaves other labs untouched. */
export function beginDeploy(labId: string, op?: string) {
  flushDeployLogs();
  setRun(labId, { busy: true, logs: [], times: [], op, startedAt: Date.now() });
}

/** Ends `labId`'s log on the error it failed with: a ✗ line in the player's language, and the
 *  error's code kept beside it for the hints. */
export function failDeploy(labId: string, e: unknown) {
  appendDeployLog(labId, `✗ ${describeError(e)}`);
  flushDeployLogs();
  const prev = state.runs[labId];
  if (prev) setRun(labId, { ...prev, failureCode: errorCode(e) });
}

/** The failure that ended a lab's run (its ✗ line, without the mark), once the run is over;
 *  null while it runs or when it went through. */
export function runFailure(run: DeployRun | undefined): string | null {
  if (!run || run.busy) return null;
  const line = run.logs.findLast((l) => l.startsWith("✗"));
  return line ? line.replace(/^✗\s*/, "") : null;
}

/** Whether a lab's last run was a start (a launch) that failed. */
export const startFailed = (run: DeployRun | undefined) => run?.op === "launch" && runFailure(run) !== null;

/** Append a log line to `labId`'s run, timestamped on arrival (shown with the next batch). */
export function appendDeployLog(labId: string, line: string) {
  const batch = pending.get(labId) ?? { lines: [], times: [] };
  batch.lines.push(line);
  batch.times.push(Date.now());
  pending.set(labId, batch);
  if (flushTimer === null) flushTimer = setTimeout(flushDeployLogs, FLUSH_MS);
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
  if (run) {
    setRun(labId, run);
    return;
  }
  if (!(labId in state.runs)) return;
  state = { runs: Object.fromEntries(Object.entries(state.runs).filter(([id]) => id !== labId)) };
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

/** The runs, re-rendering on each change. */
export function useDeploy(): DeployState {
  return useSyncExternalStore(subscribeDeploy, snapshot, snapshot);
}
