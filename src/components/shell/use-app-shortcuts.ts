"use client";

import { useEffect, useEffectEvent } from "react";
import { isTyping, shortcutFor } from "@/components/shell/shell-model";
import { isMac } from "@/lib/platform";

/** The main window's keyboard shortcuts: the command palette, Settings and the lab search. */
export function useAppShortcuts({ togglePalette, openSettings, findLab }: { togglePalette: () => void; openSettings: () => void; findLab: () => void }) {
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    const shortcut = shortcutFor(e, isMac(), isTyping(document.activeElement));
    if (!shortcut) return;
    // Not while a modal dialog is open (the quit guard, a launch confirmation, a lab's removal),
    // wherever the focus is: it owns the keyboard, and "/" would switch the screen behind it.
    // The palette's own shortcut still closes an open palette.
    const paletteOpen = shortcut === "palette" && !!document.querySelector("[data-command-palette]");
    if (!paletteOpen && document.querySelector('[aria-modal="true"]:not([data-command-palette])')) return;
    e.preventDefault();
    if (shortcut === "palette") togglePalette();
    else if (shortcut === "settings") openSettings();
    else findLab();
  });
  useEffect(() => {
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
