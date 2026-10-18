"use client";

import { useSyncExternalStore } from "react";

/** Whether the app runs on macOS (false outside a browser). */
export const isMac = () => typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

const noSubscription = () => () => {};

/** `isMac()` for rendering: false on the server render, then the real answer. */
export function useIsMac() {
  return useSyncExternalStore(noSubscription, isMac, () => false);
}
