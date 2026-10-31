"use client";

import { useSyncExternalStore } from "react";
import { authLogin, authLoginCancel, authLoginDevice, type AuthStatus, type DeviceCode } from "@/lib/tauri";
import { describeError, errorCode } from "@/lib/errors";
import { registerReset } from "@/lib/module-state";

// The sign-in, one at a time for the whole window: the sidebar, Settings, the onboarding and the
// lab pages all start it here and show the same wait, so a second click elsewhere joins it
// instead of starting another one on another callback port. While it waits, the sign-in link can
// be copied (a browser that takes the link and shows nothing can't be detected), the sign-in
// switched to a code entered on another device, or cancelled.

export type SignInState = {
  /** Waiting for the player to finish in the browser, or to approve the code. */
  waiting: boolean;
  /** The sign-in link, once the browser was asked to open it. */
  url: string | null;
  /** The code to enter on another device, once a sign-in with a code got one. */
  code: DeviceCode | null;
  /** Why the last sign-in failed (not a cancel). */
  error: string | null;
};

const IDLE: SignInState = { waiting: false, url: null, code: null, error: null };

let state: SignInState = IDLE;
let running: Promise<AuthStatus | null> | null = null;
let runningWithCode = false;
/** The sign-in whose outcome the state shows: an earlier one ending late changes nothing. */
let generation = 0;
const listeners = new Set<() => void>();

function set(next: SignInState) {
  state = next;
  listeners.forEach((l) => {
    l();
  });
}

/** Starts a sign-in (`start` gets a setter for what it streams back), replacing the state. */
function run(withCode: boolean, start: (update: (part: Partial<SignInState>) => void) => Promise<AuthStatus>): Promise<AuthStatus | null> {
  const gen = ++generation;
  const current = () => gen === generation;
  const update = (part: Partial<SignInState>) => {
    if (current()) set({ ...state, ...part });
  };
  set({ ...IDLE, waiting: true });
  const signingIn: Promise<AuthStatus | null> = start(update)
    .then(
      (status) => {
        if (current()) set(IDLE);
        return status;
      },
      (e: unknown) => {
        if (errorCode(e) === "signInCancelled") {
          // Replaced by a sign-in with a code: whoever waited on this one gets that one's outcome.
          if (!current()) return running;
          set(IDLE);
          return null;
        }
        if (current()) set({ ...IDLE, error: describeError(e) });
        throw e;
      },
    )
    .finally(() => {
      if (running === signingIn) running = null;
    });
  running = signingIn;
  runningWithCode = withCode;
  return signingIn;
}

/**
 * Signs in through the browser. Resolves with the new status, or null when the sign-in was
 * cancelled; rejects with why it failed (also kept in the state). A call while one waits joins it.
 */
export function signIn(): Promise<AuthStatus | null> {
  if (running) return running;
  return run(false, (update) =>
    authLogin((url) => {
      update({ url });
    }),
  );
}

/**
 * Signs in with a code entered on any device, for when this machine's browser can't be used. A
 * browser sign-in waiting ends, and whoever waited on it gets this one's outcome.
 */
export function signInWithCode(): Promise<AuthStatus | null> {
  if (running && runningWithCode) return running;
  return run(true, (update) =>
    authLoginDevice((code) => {
      update({ code });
    }),
  );
}

/** Ends the sign-in waiting: `signIn` resolves with null, nothing is shown. */
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

// No sign-in waiting, nothing failed (see registerReset).
registerReset(() => {
  running = null;
  runningWithCode = false;
  generation++;
  set(IDLE);
});
