"use client";

import { useCallback, useSyncExternalStore } from "react";

/** Whether `query` (a CSS media query) matches now, following the window as it is resized.
 *  False on the server and where `matchMedia` is missing. */
export function useMediaQuery(query: string) {
  const subscribe = useCallback(
    (changed: () => void) => {
      if (typeof window === "undefined" || !window.matchMedia) return () => {};
      const list = window.matchMedia(query);
      list.addEventListener("change", changed);
      return () => list.removeEventListener("change", changed);
    },
    [query],
  );
  const read = () => typeof window !== "undefined" && !!window.matchMedia?.(query).matches;
  return useSyncExternalStore(subscribe, read, () => false);
}
