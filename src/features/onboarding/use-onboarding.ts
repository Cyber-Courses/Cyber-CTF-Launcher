"use client";

import { useEffect, useState } from "react";
import { useAuthActions } from "@/features/account/account";
import type { AuthStatus } from "@/lib/tauri";
import { readSessionOnce } from "@/components/shell/use-auth-session";

/** Who is signed in (read once on mount), and the app's one sign-in: on failure the step stays
 *  and says why (a timeout, a missing browser or an unsaved session). */
export function useOnboardingAuth() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const { login, busy, error } = useAuthActions(setAuth);
  useEffect(() => {
    readSessionOnce(setAuth);
  }, []);
  return { auth, loggingIn: busy, loginError: error, login };
}
