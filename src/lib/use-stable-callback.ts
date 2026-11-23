"use client";

import { useCallback, useLayoutEffect, useRef } from "react";

/**
 * A function that keeps its identity across renders and always calls the latest `fn`: for a
 * handler handed to memoized list rows, which would otherwise all re-render whenever the parent
 * does. Not for calling during render.
 */
export function useStableCallback<A extends unknown[], R>(fn: (...args: A) => R): (...args: A) => R {
  const latest = useRef(fn);
  useLayoutEffect(() => {
    latest.current = fn;
  });
  return useCallback((...args: A) => latest.current(...args), []);
}
