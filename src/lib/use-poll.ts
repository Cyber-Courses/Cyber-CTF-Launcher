"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { structurallyEqual } from "@/lib/equal";
import { ignore } from "@/lib/failure";
import { isVisible, onVisibilityChange } from "@/lib/visibility";

export interface PollOptions<T> {
  /** Each answer, unless the poll stopped or restarted while it was being read, or a read
   *  started after it already answered. */
  onValue: (value: T) => void;
  /** A failed read; by default ignored, since the next tick reads again. */
  onError?: (e: unknown) => void;
  /** Skip a tick while the previous read is still in flight: slow reads must not stack. On by
   *  default; a read asked for (`readNow`, the window showing again) always starts. */
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

/**
 * Reads `read()` now and then every `ms` while enabled and the window is visible. The latest
 * `read` and callbacks are used on each tick without restarting the poll. Returns a function
 * that reads once more right away.
 */
export function usePoll<T>(
  read: () => Promise<T>,
  ms: number,
  { onValue, onError, serial = true, onFocus = false, enabled = true, restartKey, equal, background = false, leading = true }: PollOptions<T>,
) {
  const latest = useRef({ read, onValue, onError, equal });
  useEffect(() => {
    latest.current = { read, onValue, onError, equal };
  });
  const now = useRef<() => void>(() => {});

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    let reading = 0;
    // Reads are numbered: only the newest one started may deliver, so a slow read can't land
    // after a newer one with older news.
    let started = 0;
    let landed = 0;
    let last: { value: T } | null = null;
    let timer: ReturnType<typeof setInterval> | undefined;
    const readOnce = (force: boolean) => {
      if (serial && reading > 0 && !force) return;
      const mine = ++started;
      reading++;
      latest.current
        .read()
        .then(
          (v) => {
            if (!alive || mine < landed) return;
            landed = mine;
            const eq = latest.current.equal;
            if (eq && last && eq(last.value, v)) return;
            last = { value: v };
            latest.current.onValue(v);
          },
          (e) => alive && (latest.current.onError ?? IGNORE)(e),
        )
        .finally(() => {
          reading--;
        });
    };
    const tick = () => readOnce(false);
    const run = () => {
      if (timer === undefined) timer = setInterval(tick, ms);
    };
    const pause = () => {
      clearInterval(timer);
      timer = undefined;
    };
    now.current = () => readOnce(true);
    if (background || isVisible()) {
      if (leading) tick();
      run();
    }
    const offVisibility = background
      ? () => {}
      : onVisibilityChange((visible) => {
          if (!visible) return pause();
          readOnce(true);
          run();
        });
    if (onFocus) window.addEventListener("focus", tick);
    return () => {
      alive = false;
      now.current = () => {};
      pause();
      offVisibility();
      if (onFocus) window.removeEventListener("focus", tick);
    };
  }, [ms, serial, onFocus, enabled, restartKey, background, leading]);

  return useCallback(() => now.current(), []);
}

/** `usePoll` keeping the latest answer as state: `[value, readNow]`. An answer equal to the
 *  current one (`equal`, structural by default) keeps the current object, so a poll reading the
 *  same thing again re-renders nothing. */
export function usePolled<T>(read: () => Promise<T>, ms: number, initial: T, { equal = structurallyEqual, ...options }: Omit<PollOptions<T>, "onValue"> = {}) {
  const [value, setValue] = useState<T>(initial);
  const readNow = usePoll(read, ms, { ...options, onValue: (v) => setValue((prev) => (equal(prev, v) ? prev : v)) });
  return [value, readNow] as const;
}
