"use client";

import { useEffect, useState } from "react";

/**
 * The launcher's appearance: Dark (default), Black (true #000, for OLED) or Light (Paper).
 * Saved on this computer and applied as a class on <html> (see globals.css). public/appearance.js
 * applies it before first paint; every window follows a change made in another one through the
 * `storage` event.
 */
export type Appearance = "dark" | "black" | "light";

const KEY = "cyberctf.appearance";
export const APPEARANCES: { value: Appearance; label: string }[] = [
  { value: "dark", label: "Dark" },
  { value: "black", label: "Black" },
  { value: "light", label: "Light" },
];

function parse(v: string | null): Appearance {
  return v === "black" || v === "light" ? v : "dark";
}

function getAppearance(): Appearance {
  try {
    return parse(localStorage.getItem(KEY));
  } catch {
    return "dark";
  }
}

function applyAppearance(mode: Appearance) {
  const root = document.documentElement;
  root.classList.remove("black", "light");
  if (mode !== "dark") root.classList.add(mode);
  applyWindowTheme(mode);
}

/** The window's own frame follows the appearance (on Windows the title bar is otherwise the
 *  system's colour: light over a dark app). No-op outside the desktop app. */
function applyWindowTheme(mode: Appearance) {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
  import("@tauri-apps/api/window").then(({ getCurrentWindow }) => getCurrentWindow().setTheme(mode === "light" ? "light" : "dark")).catch(() => {});
}

function setAppearance(mode: Appearance) {
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    /* kept for this session only */
  }
  applyAppearance(mode);
}

/** The current appearance, following changes made here or in another window. */
export function useAppearance(): [Appearance, (mode: Appearance) => void] {
  const [mode, setMode] = useState<Appearance>("dark");
  useEffect(() => {
    // Read once on mount (localStorage isn't available during render).
    const current = getAppearance();
    // eslint-disable-next-line react-hooks/set-state-in-effect -- localStorage isn't readable during a server render, so the saved choice can only arrive after mount
    setMode(current);
    // public/appearance.js set the page's colours before paint; the window frame follows here.
    applyWindowTheme(current);
    const onStorage = (e: StorageEvent) => {
      if (e.key !== KEY) return;
      const next = parse(e.newValue);
      applyAppearance(next);
      setMode(next);
    };
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener("storage", onStorage);
    };
  }, []);
  return [
    mode,
    (next) => {
      setAppearance(next);
      setMode(next);
    },
  ];
}

/** Keeps this window in step with an appearance chosen in another window (Settings). */
export function useAppearanceSync() {
  useAppearance();
}
