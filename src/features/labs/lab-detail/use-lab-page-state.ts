"use client";

import { useState } from "react";
import { deriveLabState, type LabOperation, type LabStateInput } from "@/features/labs/lab-detail/lab-state";
import { failureKind } from "@/features/labs/failure-hints";
import type { ErrorCode } from "@/lib/errors";
import { operationSeenAt, useActiveOperations, useDeployingLabs } from "@/features/labs/runs/operations";
import { useWorkerLog } from "@/features/labs/runs/worker-log";
import { failureOf } from "@/lib/log-marks";
import type { SaidStep } from "@/lib/tauri/run-events";

const NO_MARKS: SaidStep[] = [];

/** The failed operation's id (none while one runs): this session's run's when it has the log, else
 *  the replayed one's. */
function failedOperation(deploying: boolean, local: boolean, run: string | null | undefined, replayed: string | null): string | null {
  if (deploying) return null;
  return local ? (run ?? null) : replayed;
}

/** When this window first saw the backend's operation `op` in flight on the lab (none: none). */
const backendSince = (labId: string, op: LabOperation | undefined) => (op ? operationSeenAt(labId, op) : undefined);

/**
 * The lab page's derived state (see deriveLabState), with what the backend reports in flight
 * on the lab, and the deploy log to show: this session's own log of the run when it has one;
 * else the worker's log file, for a deploy that kept running through a reload or relaunch.
 * With no log at all (an operation run elsewhere), the step the backend reports and when this
 * window first saw the operation stand in, so the page says what the sidebar says.
 */
export function useLabPageState(
  labId: string,
  {
    logs,
    times,
    marks,
    runOp,
    runStartedAt,
    runFailureCode,
    runFailureHolder,
    runFailureOperation,
    ...input
  }: Omit<LabStateInput, "backendDeploying" | "backendOp" | "startFailed" | "runOp"> & {
    logs: string[];
    times: number[];
    /** The steps the runner said among `logs`. */
    marks?: SaidStep[];
    /** What this session's last run on the lab was (launch, stop…), and when it began. */
    runOp?: LabOperation;
    runStartedAt?: number;
    runFailureCode?: ErrorCode | null;
    runFailureHolder?: string | null;
    runFailureOperation?: string | null;
  },
) {
  // Labs the backend is still deploying, so a window reload recovers the "starting" state.
  const backendDeploying = useDeployingLabs().has(labId);
  // An operation the runner service runs on the lab after this page reloaded (or the app
  // relaunched): no local log of it, so follow its lines in the run events instead.
  const worker = useWorkerLog(labId, backendDeploying && !input.busy && logs.length === 0);
  const active = useActiveOperations().get(labId);
  const backendOp = active?.op;
  // The panel keeps showing the last run's log once it's over (a resume's, say): keep reading
  // it as that operation.
  const [lastOperation, setLastOperation] = useState<LabOperation>("launch");
  const local = logs.length > 0 || input.busy;
  const shownLogs = local ? logs : worker.lines;
  const failure = failureOf(shownLogs);
  // Which operation failed: this session's run says; a worker's log is of the operation last seen.
  const failedOp = failure ? (runOp ?? lastOperation) : null;
  // The clicked operation names the state until the backend reports it (a clean-up read as
  // "Starting…" until the next poll).
  const state = deriveLabState({ ...input, backendDeploying, backendOp, runOp, startFailed: failedOp === "launch" });
  if (state.operation && state.operation !== lastOperation) setLastOperation(state.operation);
  return {
    ...state,
    lastOperation,
    shownLogs,
    /** The deploy log the panel shows: its lines, when each came, and the runner's steps among them. */
    deployLog: local ? { lines: logs, times, marks: marks ?? NO_MARKS } : { lines: worker.lines, times: worker.times, marks: worker.marks },
    // A run that ended in ✗ failed.
    deployFailed: failure !== null,
    /** The failure the last run ended on, and which operation it was (null while one runs). */
    failure: state.deploying ? null : failure,
    failedOp: state.deploying ? null : failedOp,
    /** What kind of failure it is, for a hint: by this session's error code, else its words. */
    failureKind: state.deploying ? null : failureKind(failure, local ? runFailureCode : worker.failureCode),
    /** For a port clash this session saw, what holds the host port (so the notice can name it). */
    failureHolder: state.deploying || !local ? null : (runFailureHolder ?? null),
    /** The id of the operation that failed (this session's, else the replayed one's), to quote. */
    failureOperation: failedOperation(state.deploying, local, runFailureOperation, worker.failureOperation),
    /** When the operation in flight (or the last one) began: this session's run, else when this
     *  window first saw the backend's. */
    startedAt: local ? runStartedAt : backendSince(labId, backendOp),
    /** The backend's step for the operation in flight, when there is no log to read steps from. */
    liveStep: !local && worker.lines.length === 0 ? (active?.step ?? null) : null,
  };
}
