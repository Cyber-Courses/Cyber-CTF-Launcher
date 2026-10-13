"use client";

import { useState } from "react";
import { deriveLabState, type LabOperation, type LabStateInput } from "@/features/labs/lab-detail/lab-state";
import { operationSeenAt, useActiveOperations, useDeployingLabs, useWorkerLog } from "@/lib/deploy-store";

/** The failure a log ended on (its last ✗ line, without the mark), or null. */
export function logFailure(lines: string[]): string | null {
  const line = lines.findLast((l) => l.startsWith("✗"));
  return line ? line.replace(/^✗\s*/, "") : null;
}

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
    runOp,
    runStartedAt,
    ...input
  }: Omit<LabStateInput, "backendDeploying" | "backendOp" | "startFailed" | "runOp"> & {
    logs: string[];
    times: number[];
    /** What this session's last run on the lab was (launch, stop…), and when it began. */
    runOp?: string;
    runStartedAt?: number;
  },
) {
  // Labs the backend is still deploying, so a window reload recovers the "starting" state.
  const backendDeploying = useDeployingLabs().has(labId);
  // A deploy running in its worker process after this page reloaded (or the app relaunched): no
  // local log of it, so follow the worker's log file instead.
  const worker = useWorkerLog(labId, backendDeploying && !input.busy && logs.length === 0);
  const active = useActiveOperations().get(labId);
  const backendOp = active?.op;
  // The panel keeps showing the last run's log once it's over (a resume's, say): keep reading
  // it as that operation.
  const [lastOperation, setLastOperation] = useState<LabOperation>("launch");
  const local = logs.length > 0 || input.busy;
  const shownLogs = local ? logs : worker.lines;
  const failure = logFailure(shownLogs);
  // Which operation failed: this session's run says; a worker's log is of the operation last seen.
  const failedOp = failure ? ((runOp as LabOperation | undefined) ?? lastOperation) : null;
  // The clicked operation names the state until the backend reports it (a clean-up read as
  // "Starting…" until the next poll).
  const state = deriveLabState({ ...input, backendDeploying, backendOp, runOp: runOp as LabOperation | undefined, startFailed: failedOp === "launch" });
  if (state.operation && state.operation !== lastOperation) setLastOperation(state.operation);
  return {
    ...state,
    lastOperation,
    shownLogs,
    shownTimes: local ? times : worker.times,
    // A run that ended in ✗ failed.
    deployFailed: failure !== null,
    /** The failure the last run ended on, and which operation it was (null while one runs). */
    failure: state.deploying ? null : failure,
    failedOp: state.deploying ? null : failedOp,
    /** When the operation in flight (or the last one) began: this session's run, else when this
     *  window first saw the backend's. */
    startedAt: local ? runStartedAt : backendOp ? operationSeenAt(labId, backendOp) : undefined,
    /** The backend's step for the operation in flight, when there is no log to read steps from. */
    liveStep: !local && worker.lines.length === 0 ? (active?.step ?? null) : null,
  };
}
