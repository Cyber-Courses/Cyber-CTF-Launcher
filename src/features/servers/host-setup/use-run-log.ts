"use client";

import { useCallback, useState } from "react";
import { describeError } from "@/lib/errors";

/**
 * The streamed log of an install or a sign-in (null before the first run), and `run()` to start
 * one: the log restarts at `first`, collects every line `task` streams, and ends with the line
 * `task` returns, or "✗ <why>" on failure. `run()` never rejects: the log says how it went.
 */
export function useRunLog() {
  const [lines, setLines] = useState<string[] | null>(null);
  const append = useCallback((l: string) => {
    setLines((x) => [...(x ?? []), l]);
  }, []);
  const run = useCallback(
    async (first: string, task: (onLog: (l: string) => void) => Promise<string>) => {
      setLines([first]);
      try {
        append(await task(append));
      } catch (e) {
        append(`✗ ${describeError(e)}`);
      }
    },
    [append],
  );
  return { lines, run };
}
