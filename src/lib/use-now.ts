"use client";

import { useCallback, useEffect, useState } from "react";

/**
 * The current time, re-read every `ms` while `enabled`, for relative labels ("5 min ago") and
 * running counters. Returns `[now, touch]`; `touch` re-reads it right away.
 */
export function useNow(ms: number, enabled = true) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!enabled) return;
    const timer = setInterval(() => {
      setNow(Date.now());
    }, ms);
    return () => {
      clearInterval(timer);
    };
  }, [ms, enabled]);
  const touch = useCallback(() => {
    setNow(Date.now());
  }, []);
  return [now, touch] as const;
}
