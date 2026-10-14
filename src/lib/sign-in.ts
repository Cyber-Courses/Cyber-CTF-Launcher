"use client";

import { useSyncExternalStore } from "react";
import { authLogin, authLoginCancel, type AuthStatus } from "@/lib/tauri";
import { describeError, errorCode } from "@/lib/errors";

// The sign-in through the browser, one at a time for the whole window: the sidebar, Settings,
// the onboarding and the lab pages all start it here and show the same wait, so a second click
// elsewhere joins it instead of starting another one on another callback port. While it waits,
// the sign-in link can be copied (a browser that takes the link and shows nothing can't be
// detected) and the sign-in cancelled.

export type SignInState = {
  /** Waiting for the player to finish in the browser. */
  waiting: boolean;
  /** The sign-in link, once the browser was asked to open it. */
  url: string | null;
  /** Why the last sign-in failed (not a cancel). */
  error: string | null;
};

const IDLE: SignInState = { waiting: false, url: null, error: null };

let state: SignInState = IDLE;
let running: Promise<AuthStatus | null> | null = null;
const listeners = new Set<() => void>();

function set(next: SignInState) {
  state = next;
  listeners.forEach((l) => l());
}

/**
 * Signs in through the browser. Resolves with the new status, or null when the sign-in was
 * cancelled; rejects with why it failed (also kept in the state). A call while one waits joins it.
 */
export function signIn(): Promise<AuthStatus | null> {
  if (running) return running;
  set({ ...IDLE, waiting: true });
  running = authLogin((url) => set({ ...state, url }))
    .then(
      (status) => {
        set(IDLE);
        return status;
      },
      (e: unknown) => {
        // Cancelled (here, or by another sign-in): not a failure to show.
        if (errorCode(e) === "signInCancelled") {
          set(IDLE);
          return null;
        }
        set({ ...IDLE, error: describeError(e) });
        throw e;
      },
    )
    .finally(() => {
      running = null;
    });
  return running;
}

/** Ends the sign-in waiting for the browser: `signIn` resolves with null, nothing is shown. */
export const cancelSignIn = () => authLoginCancel();

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** The sign-in's state, for the views that start it. */
export function useSignIn(): SignInState {
  return useSyncExternalStore(
    subscribe,
    () => state,
    () => IDLE,
  );
}

/** Tests: no sign-in waiting, nothing failed. */
export function resetSignIn() {
  running = null;
  set(IDLE);
}
