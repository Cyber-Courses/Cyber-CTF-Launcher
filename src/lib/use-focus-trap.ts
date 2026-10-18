"use client";

import { useEffect, type RefObject } from "react";
import { useEscape } from "@/lib/use-escape";

const FOCUSABLE = 'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

/**
 * Traps keyboard focus inside `ref` while `active`: focuses the first focusable element on open,
 * cycles Tab / Shift+Tab within the container (so focus can't escape to the page behind a modal),
 * and restores focus to the previously focused element on close. A no-op when inactive.
 * `onEscape`, when given, runs on Escape while the trap is active (the dialog's "cancel"), see
 * useEscape.
 */
export function useFocusTrap(ref: RefObject<HTMLElement | null>, active = true, onEscape?: () => void) {
  useEscape(() => onEscape?.(), active && !!onEscape);
  useEffect(() => {
    const el = ref.current;
    if (!active || !el) return;
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const focusable = () => Array.from(el.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((n) => n.offsetParent !== null);
    (focusable()[0] ?? el).focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const f = focusable();
      if (f.length === 0) {
        e.preventDefault();
        return;
      }
      const first = f[0];
      const last = f[f.length - 1];
      const current = document.activeElement;
      if (e.shiftKey && (current === first || !el.contains(current))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (current === last || !el.contains(current))) {
        e.preventDefault();
        first.focus();
      }
    };
    el.addEventListener("keydown", onKey);
    return () => {
      el.removeEventListener("keydown", onKey);
      // Restore focus to where it was before the dialog opened (if still in the document).
      if (previouslyFocused && document.contains(previouslyFocused)) previouslyFocused.focus();
    };
  }, [ref, active]);
}
