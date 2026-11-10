"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { structurallyEqual } from "@/lib/equal";
import { ignore } from "@/lib/failure";
import { createPoll } from "@/lib/shared-poll";

export interface PollOptions<T> {
  /** Each answer, unless the poll stopped or restarted while it was being read, or a read
   *  started after it already answered. */
  onValue: (value: T) => void;
  /** A failed read; by default ignored, since the next read comes in a moment. */
  onError?: (e: unknown) => void;
  /** One read at a time, the next timed from the end of the previous: slow reads must not
   *  stack. On by default; false reads every `ms` from the previous read's start. A read asked
   *  for (`readNow`, focus, the window showing again) always starts. */
  serial?: boolean;
  /** Also read when the window regains focus. */
  onFocus?: boolean;
  /** False pauses the poll. */
  enabled?: boolean;
  /** A change restarts the poll, with an immediate read. */
  restartKey?: unknown;
  /** Skip `onValue` when an answer equals the last one delivered. */
  equal?: (a: T, b: T) => boolean;
  /** Keep polling while the window is hidden (by default the poll pauses, and reads at once
   *  when the window shows again). */
  background?: boolean;
  /** Read at once when the poll starts (default), or only after the first interval. */
  leading?: boolean;
}

const IGNORE = ignore("polled again in a moment");

type PollCallbacks<T> = Pick<PollOptions<T>, "onValue" | "onError" | "equal"> & { read: () => Promise<T> };

/**
 * Reads `read()` now and then every `ms` while enabled and the window is visible: a poll of its
 * own for this component, on the same engine as the window's shared ones (`createPoll`). The
 * latest `read` and callbacks are used on each read without restarting the poll. Returns a
 * function that reads once more right away, resolving once that read has answered or failed (a
 * Try again spins until then).
 */
export function usePoll<T>(
  read: () => Promise<T>,
  ms: number,
  { onValue, onError, serial = true, onFocus = false, enabled = true, restartKey, equal, background = false, leading = true }: PollOptions<T>,
) {
  const latest = useRef<PollCallbacks<T>>({ read, onValue, onError, equal });
  useEffect(() => {
    latest.current = { read, onValue, onError, equal };
  });
  const now = useRef<() => Promise<void>>(() => Promise.resolve());

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    // Each answer boxed, so every one is delivered (the engine publishes only a change), unless
    // `equal` says it's the last one again.
    const poll = createPoll<{ value: T } | null>({
      read: () => latest.current.read().then((value) => ({ value })),
      ms,
      initial: null,
      serial,
      onFocus,
      leading,
      hiddenMs: background ? () => ms : undefined,
      equal: (a, b) => a !== null && b !== null && (latest.current.equal?.(a.value, b.value) ?? false),
      onError: (e) => {
        if (alive) (latest.current.onError ?? IGNORE)(e);
      },
    });
    const off = poll.subscribe(() => {
      const answer = poll.get();
      if (answer) latest.current.onValue(answer.value);
    });
    now.current = poll.refresh;
    return () => {
      alive = false;
      now.current = () => Promise.resolve();
      off();
    };
  }, [ms, serial, onFocus, enabled, restartKey, background, leading]);

  return useCallback(() => now.current(), []);
}

/** `usePoll` keeping the latest answer as state: `[value, readNow]`. An answer equal to the
 *  current one (`equal`, structural by default) keeps the current object, so a poll reading the
 *  same thing again re-renders nothing. */
export function usePolled<T>(read: () => Promise<T>, ms: number, initial: T, { equal = structurallyEqual, ...options }: Omit<PollOptions<T>, "onValue"> = {}) {
  const [value, setValue] = useState<T>(initial);
  const readNow = usePoll(read, ms, {
    ...options,
    onValue: (v) => {
      setValue((prev) => (equal(prev, v) ? prev : v));
    },
  });
  return [value, readNow] as const;
}
