"use client";

import { useRef, useState } from "react";
import { systemCheck, type SystemReport } from "@/lib/tauri";
import { usePoll } from "@/lib/use-poll";
import { machineCheckInterval } from "@/features/machine/check-cadence";
import { describeError } from "@/lib/errors";
import { warn } from "@/lib/failure";

/**
 * The machine report for a setup (the onboarding, the machine-setup window), polled so an install
 * finished in a native installer is picked up without a re-check: on its own cadence, when the
 * window comes back, and on `refreshReport` (after an install). A failed re-check keeps the last
 * report: clearing it blanked the current step to the loader and, on Windows, dropped the WSL
 * step from the list, so the player landed on another step. A check that failed before any
 * answered is said (`checkError`) and polled no more, rather than a loader over silent retries:
 * `retry` checks once, and starts the poll again once it answers.
 */
export function useMachineReport() {
  const [report, setReport] = useState<SystemReport | null>(null);
  const [checkError, setCheckError] = useState<string | null>(null);
  const failing = useRef(false);
  const answered = (r: SystemReport) => {
    failing.current = false;
    setReport(r);
    setCheckError(null);
  };
  const failed = (e: unknown) => {
    if (!failing.current) warn("system check")(e);
    failing.current = true;
    setCheckError(describeError(e));
  };
  const refreshReport = usePoll(systemCheck, machineCheckInterval(checkError !== null), {
    serial: true,
    onFocus: true,
    onValue: answered,
    onError: failed,
  });
  return { report, checkError, refreshReport };
}

export type MachineReport = ReturnType<typeof useMachineReport>;
