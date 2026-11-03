"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { authStatus, type AuthStatus } from "@/lib/tauri";
import { AUTH_CHANGED_EVENT } from "@/lib/app-events";
import { SIGNED_OUT_EVENT } from "@/features/account/signed-out";
import { ignore, warn } from "@/lib/failure";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { usePoll } from "@/lib/use-poll";
import { structurallyEqual } from "@/lib/equal";
import { toast } from "@/components/ui/toaster";
import { translate } from "@/lib/i18n";

const SIGNED_IN_POLL_MS = 30_000;
const SIGNED_OUT: AuthStatus = { loggedIn: false, name: null, email: null };

/**
 * The player's session, as this window knows it (null until first read).
 *
 * The session can disappear under the app (signed out elsewhere, a refresh token revoked, the
 * keychain locked or cleared). It is read again when an action says so, when the window comes
 * back, and now and then while signed in, so the sidebar and Start buttons don't claim a session
 * that's gone.
 */
export function useAuthSession() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  // The session as last rendered, to notice it going away under the app (a re-read that comes
  // back signed out with the session expired, after one signed in).
  const last = useRef<AuthStatus | null>(null);
  useEffect(() => {
    last.current = auth;
  }, [auth]);
  // An answer equal to the session known keeps it as is: nothing reading it re-renders.
  const keep = useCallback((s: AuthStatus) => {
    // Signed out under the app, not by the player: said once here (the sidebar and Settings
    // keep saying why until the next sign-in).
    if (last.current?.loggedIn && !s.loggedIn && s.sessionExpired) toast(translate("account.sessionExpiredTitle"), translate("account.sessionExpired"), "warn");
    setAuth((prev) => (structurallyEqual(prev, s) ? prev : s));
  }, []);

  /** Reads the session again; a failure keeps the last one known. */
  const reread = useCallback(() => void authStatus().then(keep).catch(ignore("read again on the next sign-out event or launch")), [keep]);

  const loggedIn = !!auth?.loggedIn;
  useEffect(() => {
    window.addEventListener(SIGNED_OUT_EVENT, reread);
    window.addEventListener("focus", reread);
    return () => {
      window.removeEventListener(SIGNED_OUT_EVENT, reread);
      window.removeEventListener("focus", reread);
    };
  }, [reread]);
  // Now and then while signed in (not at once: the session was just read), paused while the
  // window is hidden and read again when it shows.
  usePoll(authStatus, SIGNED_IN_POLL_MS, {
    enabled: loggedIn,
    leading: false,
    onValue: keep,
    onError: ignore("read again on the next sign-out event or launch"),
  });
  // Signing in or out in the Settings window: `focus` doesn't fire reliably when moving between
  // the app's own windows, so the other windows hear it as an app event.
  useTauriEvent(AUTH_CHANGED_EVENT, reread);

  /** Signed in or out here: the other windows (Settings, if open) hear it too. */
  const changed = useCallback((status: AuthStatus) => {
    setAuth(status);
    emit(AUTH_CHANGED_EVENT).catch(warn("Couldn't tell the other windows about the sign-in"));
  }, []);

  /** The first read: a failure reads as signed out. */
  const init = useCallback(
    () =>
      void authStatus()
        .then(setAuth)
        .catch(() => {
          setAuth(SIGNED_OUT);
        }),
    [],
  );

  return { auth, setAuth, changed, init, reread };
}
