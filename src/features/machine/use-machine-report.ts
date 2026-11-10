"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { systemCheck, type SystemReport } from "@/lib/tauri";
import { usePoll } from "@/lib/use-poll";
import { machineCheckInterval } from "@/features/machine/check-cadence";
import { describeError } from "@/lib/errors";
import { warn } from "@/lib/failure";
import { useLatest } from "@/lib/use-latest";

/** At most one re-check per this long when the app shell's window comes back. */
const FOCUS_RECHECK_MS = 15_000;

/**
 * The machine report and why its last check failed, as both readers below keep them: a failed
 * re-check keeps the last report (clearing it blanked a setup's current step to the loader and,
 * on Windows, dropped the WSL step from the list), and a failure is logged once until a check
 * answers again.
 */
function useReportState() {
  const [report, setReport] = useState<SystemReport | null>(null);
  const [checkError, setCheckError] = useState<string | null>(null);
  const failing = useRef(false);
  const answered = useCallback((r: SystemReport) => {
    failing.current = false;
    setReport(r);
    setCheckError(null);
  }, []);
  const failed = useCallback((e: unknown) => {
    if (!failing.current) warn("system check")(e);
    failing.current = true;
    setCheckError(describeError(e));
  }, []);
  return { report, checkError, answered, failed };
}

/**
 * The machine report for a setup (the onboarding, the machine-setup window), polled so an install
 * finished in a native installer is picked up without a re-check: on its own cadence, when the
 * window comes back, and on `refreshReport` (after an install). A check that failed is tried again
 * on the failing cadence, and `checkError` says why meanwhile.
 */
export function useMachineReport() {
  const { report, checkError, answered, failed } = useReportState();
  const refreshReport = usePoll(systemCheck, machineCheckInterval(checkError !== null), {
    serial: true,
    onFocus: true,
    onValue: answered,
    onError: failed,
  });
  return { report, checkError, refreshReport };
}

export type MachineReport = ReturnType<typeof useMachineReport>;

/**
 * The machine report for the app shell (engines, hypervisors, tools): read when asked (`check`)
 * and again when the window comes back, at most every 15 s, so Docker started from a terminal,
 * or an engine installed meanwhile, shows up without a restart. Not polled: the shell runs all
 * the time, and each check starts a process per tool. Only the newest check's answer is kept (a
 * slow, older "Docker isn't running" must not come back over a re-check's).
 */
export function useShellMachineReport() {
  const { report, checkError, answered, failed } = useReportState();
  const lastCheck = useRef(0);
  const begin = useLatest();
  const check = useCallback(() => {
    lastCheck.current = Date.now();
    const isLatest = begin();
    return systemCheck().then(
      (r) => {
        if (isLatest()) answered(r);
      },
      (e: unknown) => {
        if (isLatest()) failed(e);
      },
    );
  }, [begin, answered, failed]);
  useEffect(() => {
    const onFocus = () => {
      if (Date.now() - lastCheck.current > FOCUS_RECHECK_MS) void check();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
    };
  }, [check]);
  return { report, checkError, check };
}
