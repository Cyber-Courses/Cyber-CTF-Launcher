"use client";

import { useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";

/** Whether the app runs on macOS (false outside a browser). */
export const isMac = () => typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

const noSubscription = () => () => {};

/** `isMac()` for rendering: false on the server render, then the real answer. */
export function useIsMac() {
  return useSyncExternalStore(noSubscription, isMac, () => false);
}

/**
 * Who draws this window's frame. On every system the page's header row is the title bar (it drags
 * the window); what differs is who draws the window's buttons:
 * - `overlay`: macOS, where the system's traffic lights sit over the header's start;
 * - `linux`, `windows`: the page, at the header's end, where the window has no system frame
 *   (src-tauri/src/app/window.rs; src/components/window-title-bar.tsx);
 * - `system`: the browser, outside the app.
 */
export type WindowFrame = "overlay" | "linux" | "windows" | "system";

function windowFrame(): WindowFrame {
  if (isMac()) return "overlay";
  if (!isTauri()) return "system";
  return navigator.userAgent.includes("Windows") ? "windows" : "linux";
}

/** `windowFrame()` for rendering: `system` on the server render, then the real answer. */
export function useWindowFrame(): WindowFrame {
  return useSyncExternalStore(noSubscription, windowFrame, () => "system");
}
