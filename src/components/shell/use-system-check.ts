"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { systemCheck, type SystemReport } from "@/lib/tauri";
import { describeError } from "@/lib/errors";

/** At most one re-check per this long when the window comes back. */
const FOCUS_RECHECK_MS = 15_000;

/**
 * This machine's report (engines, hypervisors, tools). `check()` reads it again. Also checked
 * again when the window comes back (at most every 15 s): Docker started from a terminal, or an
 * engine installed meanwhile, shows up without a restart.
 */
export function useSystemCheck() {
  const [report, setReport] = useState<SystemReport | null>(null);
  // The check failed (and no earlier one succeeded): the screens that need it say so with a
  // retry instead of "Checking this machine…" forever.
  const [checkError, setCheckError] = useState<string | null>(null);
  const lastCheck = useRef(0);
  const check = useCallback(() => {
    lastCheck.current = Date.now();
    return systemCheck()
      .then((r) => {
        setReport(r);
        setCheckError(null);
      })
      .catch((e) => setCheckError(describeError(e)));
  }, []);
  useEffect(() => {
    const onFocus = () => {
      if (Date.now() - lastCheck.current > FOCUS_RECHECK_MS) void check();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [check]);
  return { report, checkError, check };
}
