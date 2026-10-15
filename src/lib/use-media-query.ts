"use client";

import { useCallback, useSyncExternalStore } from "react";

/** Whether this page can match media queries: not on the server, nor in a DOM without
 *  `matchMedia` (jsdom). */
const canMatch = () => typeof window !== "undefined" && typeof window.matchMedia === "function";

/** Whether `query` (a CSS media query) matches now, following the window as it is resized.
 *  False on the server and where `matchMedia` is missing. */
export function useMediaQuery(query: string) {
  const subscribe = useCallback(
    (changed: () => void) => {
      if (!canMatch()) return () => {};
      const list = window.matchMedia(query);
      list.addEventListener("change", changed);
      return () => {
        list.removeEventListener("change", changed);
      };
    },
    [query],
  );
  const read = () => canMatch() && window.matchMedia(query).matches;
  return useSyncExternalStore(subscribe, read, () => false);
}
