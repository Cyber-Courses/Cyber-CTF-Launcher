"use client";

import { useEffect, useState } from "react";
import { useAuthActions } from "@/features/account/account";
import { authStatus, systemCheck, type AuthStatus, type SystemReport } from "@/lib/tauri";
import { usePoll } from "@/lib/use-poll";
import { describeError } from "@/lib/errors";

/** Machine checks while onboarding: an install finished in a native installer shows up on its own. */
const CHECK_POLL_MS = 5000;

/** Who is signed in (read once on mount), and the app's one sign-in: on failure the step stays
 *  and says why (a timeout, a missing browser or an unsaved session). */
export function useOnboardingAuth() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const { login, busy, error } = useAuthActions(setAuth);
  useEffect(() => {
    authStatus()
      .then(setAuth)
      .catch(() => {
        setAuth(null);
      });
  }, []);
  return { auth, loggingIn: busy, loginError: error, login };
}

/**
 * The machine report, polled so an install finished in a native installer is picked up without a
 * re-check. A failed re-check keeps the last report: clearing it blanked the current step to the
 * loader and, on Windows, dropped the WSL step from the list, so the player landed on another
 * step. A check that keeps failing (a wedged daemon, a hung tool) is said, with a retry: the
 * loader alone sat there for good.
 */
export function useMachineReport() {
  const [report, setReport] = useState<SystemReport | null>(null);
  const [checkError, setCheckError] = useState<string | null>(null);
  const refreshReport = usePoll(systemCheck, CHECK_POLL_MS, {
    serial: true,
    onValue: (r) => {
      setReport(r);
      setCheckError(null);
    },
    onError: (e) => {
      setCheckError(describeError(e));
    },
  });
  return { report, checkError, refreshReport };
}

export type MachineReport = ReturnType<typeof useMachineReport>;
