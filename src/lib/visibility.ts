"use client";

/**
 * Whether the window is on screen. A hidden window (minimised, on another desktop, closed to
 * the tray, "finish in the background") shows nothing, so the polls that only feed the screen
 * pause meanwhile and read again the moment it comes back.
 */
export function isVisible(): boolean {
  return typeof document === "undefined" || document.visibilityState !== "hidden";
}

/** Calls `cb` with the new visibility each time it changes; returns an unsubscribe. */
export function onVisibilityChange(cb: (visible: boolean) => void): () => void {
  if (typeof document === "undefined") return () => {};
  const listener = () => cb(isVisible());
  document.addEventListener("visibilitychange", listener);
  return () => document.removeEventListener("visibilitychange", listener);
}
