"use client";

import { useCallback, useEffect, useRef } from "react";

/** Whether an answer may still be applied: its request is the newest of its kind, and the
 *  component that asked is still mounted. */
export type IsLatest = () => boolean;

/**
 * Orders async answers that can resolve out of order (a re-check on focus and a Check again
 * click, two Test clicks, a reload on mount and one on an event): `begin(key?)` marks a new
 * request and returns its `isLatest`, true until another request of the same key begins or the
 * component unmounts. A late, older answer is then dropped instead of overwriting a newer one.
 */
export function useLatest(): (key?: string) => IsLatest {
  const seqs = useRef(new Map<string, number>());
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  return useCallback((key = "") => {
    const mine = (seqs.current.get(key) ?? 0) + 1;
    seqs.current.set(key, mine);
    return () => alive.current && seqs.current.get(key) === mine;
  }, []);
}
