"use client";

import { useEffect, useEffectEvent } from "react";
import { isTyping, shortcutFor } from "@/components/shell/shell-model";
import { isMac } from "@/lib/platform";

/** The main window's keyboard shortcuts: the command palette, Settings and the lab search. */
export function useAppShortcuts({ togglePalette, openSettings, findLab }: { togglePalette: () => void; openSettings: () => void; findLab: () => void }) {
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    // Not inside a modal dialog (the quit guard, a launch confirmation): it owns the keyboard.
    // The palette is the exception: its own shortcut closes it.
    const modal = document.activeElement?.closest('[aria-modal="true"]');
    if (modal && !modal.hasAttribute("data-command-palette")) return;
    const shortcut = shortcutFor(e, isMac(), isTyping(document.activeElement));
    if (!shortcut) return;
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
