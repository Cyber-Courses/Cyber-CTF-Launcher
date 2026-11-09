"use client";

import { useCallback, useState } from "react";
import { describeError } from "@/lib/errors";

/**
 * The streamed log of a task the player started and watches line by line (an install, a sign-in,
 * a hypervisor add-on, the attack box going in or out of a lab; null before the first run), and
 * `run()` to start one: the log restarts at `first`, collects every line `task` streams, and ends with the line
 * `task` returns (if any), or "✗ <why>" on failure. `run()` never rejects: the log says how it
 * went.
 */
export function useRunLog() {
  const [lines, setLines] = useState<string[] | null>(null);
  const append = useCallback((l: string) => {
    setLines((x) => [...(x ?? []), l]);
  }, []);
  const run = useCallback(
    async (first: string, task: (onLog: (l: string) => void) => Promise<unknown>) => {
      setLines([first]);
      try {
        const last = await task(append);
        if (typeof last === "string") append(last);
      } catch (e) {
        append(`✗ ${describeError(e)}`);
      }
    },
    [append],
  );
  return { lines, run };
}
