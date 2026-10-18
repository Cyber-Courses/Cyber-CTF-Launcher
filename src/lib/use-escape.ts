"use client";

import { useEffect, useRef } from "react";

/**
 * Runs `onEscape` on Escape anywhere in the window while `active`: a dialog's or a menu's
 * "cancel". The key is theirs alone: stopped here, it doesn't also reach the page's own Escape
 * (the lab page goes back to the list on it), even once the focus has left them.
 */
export function useEscape(onEscape: () => void, active = true) {
  const latest = useRef(onEscape);
  useEffect(() => {
    latest.current = onEscape;
  });
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      latest.current();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, [active]);
}
