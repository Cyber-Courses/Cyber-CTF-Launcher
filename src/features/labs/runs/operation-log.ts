"use client";

import { useEffect, useMemo, useSyncExternalStore } from "react";
import { describeError, type ErrorCode } from "@/lib/errors";
import { failedLine } from "@/lib/log-marks";
import { ignore } from "@/lib/failure";
import { connectRunEvents, lastRunOf, runEventsNow, subscribeRunEvents, type SaidStep } from "@/lib/tauri/run-events";

/** A log with each line's arrival time, parallel, and the steps the runner said among them. */
export type TimedLog = { lines: string[]; times: number[]; marks: SaidStep[] };
const NO_LOG: TimedLog = { lines: [], times: [], marks: [] };

/**
 * The log of lab `labId`'s operation the core runs (or ran last), from the run events (subscribed
 * to once `active`), each line timed when this window received it (for the steps' durations),
 * kept once it ended: for a page
 * that (re)attaches to an operation after a reload or an app relaunch, which its own deploy store
 * has no lines of. A failed one ends on its ✗ line, the error's code beside.
 */
export function useOperationLog(labId: string, active: boolean): TimedLog & { failureCode: ErrorCode | null; failureOperation: string | null } {
  useEffect(() => {
    if (active) connectRunEvents().catch(ignore("the log shows once the core answers"));
  }, [active]);
  const run = useSyncExternalStore(
    subscribeRunEvents,
    () => lastRunOf(labId, runEventsNow()),
    () => undefined,
  );
  return useMemo(() => {
    if (!run) return { ...NO_LOG, failureCode: null, failureOperation: null };
    if (!run.error) return { lines: run.lines, times: run.times, marks: run.steps, failureCode: null, failureOperation: null };
    // A failed one ends on its ✗ line, in the player's language, as this window's own runs do.
    const end = run.times.at(-1) ?? run.startedAt;
    return {
      lines: [...run.lines, failedLine(describeError(run.error))],
      times: [...run.times, end],
      marks: run.steps,
      failureCode: run.error.code,
      failureOperation: run.operation,
    };
  }, [run]);
}
