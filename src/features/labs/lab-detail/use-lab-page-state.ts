"use client";

import { useState } from "react";
import { deriveLabState, type LabOperation, type LabStateInput } from "@/features/labs/lab-detail/lab-state";
import { useActiveOperations, useDeployingLabs, useWorkerLog } from "@/lib/deploy-store";

/**
 * The lab page's derived state (see deriveLabState), with what the backend reports in flight
 * on the lab, and the deploy log to show: this session's own log of the run when it has one;
 * else the worker's log file, for a deploy that kept running through a reload or relaunch.
 */
export function useLabPageState(
  labId: string,
  { logs, times, ...input }: Omit<LabStateInput, "backendDeploying" | "backendOp"> & { logs: string[]; times: number[] },
) {
  // Labs the backend is still deploying, so a window reload recovers the "starting" state.
  const backendDeploying = useDeployingLabs().has(labId);
  // A deploy running in its worker process after this page reloaded (or the app relaunched): no
  // local log of it, so follow the worker's log file instead.
  const worker = useWorkerLog(labId, backendDeploying && !input.busy && logs.length === 0);
  const backendOp = useActiveOperations().get(labId)?.op;
  const state = deriveLabState({ ...input, backendDeploying, backendOp });
  // The panel keeps showing the last run's log once it's over (a resume's, say): keep reading
  // it as that operation.
  const [lastOperation, setLastOperation] = useState<LabOperation>("launch");
  if (state.operation && state.operation !== lastOperation) setLastOperation(state.operation);
  const shownLogs = logs.length > 0 ? logs : worker.lines;
  return {
    ...state,
    lastOperation,
    shownLogs,
    shownTimes: logs.length > 0 ? times : worker.times,
    // A run that ended in ✗ failed.
    deployFailed: shownLogs.some((l) => l.startsWith("✗")),
  };
}
