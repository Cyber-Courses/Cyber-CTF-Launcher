"use client";

import { useMemo, useState } from "react";
import { labDeployLog } from "@/lib/tauri";
import { ignore } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";
import type { ErrorCode } from "@/lib/errors";
import { readFailedLine } from "@/lib/log-marks";

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
export function useWorkerLog(labId: string, active: boolean, pollMs = 1500): TimedLog & { failureCode: ErrorCode | null } {
  const [log, setLog] = useState<TimedLog>(NO_LOG);
  const [failureCode, setFailureCode] = useState<ErrorCode | null>(null);
  usePoll(() => labDeployLog(labId).then((text) => text.split("\n").filter((l) => l.length > 0)), pollMs, {
    enabled: active,
    restartKey: labId,
    onError: ignore("read again in a moment"),
    onValue: (raw) => {
      // The failed line says its error's code, which the page reads apart from the line it shows.
      const read = raw.map(readFailedLine);
      setFailureCode(read.findLast((r) => r.code !== null)?.code ?? null);
      setLog((prev) =>
        timeLines(
          prev,
          read.map((r) => r.line),
          Date.now(),
        ),
      );
    },
  });
  return useMemo(() => ({ ...log, failureCode }), [log, failureCode]);
}
