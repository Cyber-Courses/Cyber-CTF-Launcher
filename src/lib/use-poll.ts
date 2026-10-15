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

type PollCallbacks<T> = Pick<PollOptions<T>, "onValue" | "onError" | "equal"> & { read: () => Promise<T> };
type PollTiming = Required<Pick<PollOptions<unknown>, "serial" | "onFocus" | "background" | "leading">> & { ms: number };

/** The reads of one run of a poll, each answer delivered only if it is the newest read started
 *  and, with `equal`, new. `latest` gives the current read and callbacks; nothing is delivered
 *  once `stop`ped. */
function pollReads<T>(latest: () => PollCallbacks<T>, { serial }: Pick<PollTiming, "serial">) {
  let alive = true;
  let reading = 0;
  // Reads are numbered: only the newest one started may deliver, so a slow read can't land
  // after a newer one with older news.
  let started = 0;
  let landed = 0;
  let last: { value: T } | null = null;
  const deliver = (mine: number, v: T) => {
    if (!alive || mine < landed) return;
    landed = mine;
    const { equal, onValue } = latest();
    if (equal && last && equal(last.value, v)) return;
    last = { value: v };
    onValue(v);
  };
  const read = () => {
    const mine = ++started;
    reading++;
    latest()
      .read()
      .then(
        (v) => {
          deliver(mine, v);
        },
        (e: unknown) => {
          if (alive) (latest().onError ?? IGNORE)(e);
        },
      )
      .finally(() => {
        reading--;
      });
  };
  return {
    /** A tick of the timer: skipped while a read is in flight when `serial`, so slow reads don't stack. */
    tick: () => {
      if (!serial || reading === 0) read();
    },
    /** A read asked for (`readNow`, the window showing again): always starts. */
    now: read,
    stop: () => {
      alive = false;
    },
  };
}

/** One run of a poll, until `stop`: its reads every `ms`, paused while the window is hidden
 *  (unless `background`) and read again when it shows, and on focus with `onFocus`. */
function startPoll<T>(latest: () => PollCallbacks<T>, { ms, serial, onFocus, background, leading }: PollTiming) {
  const reads = pollReads(latest, { serial });
  let timer: ReturnType<typeof setInterval> | undefined;
  const { tick } = reads;
  const run = () => {
    if (timer === undefined) timer = setInterval(tick, ms);
  };
  const pause = () => {
    clearInterval(timer);
    timer = undefined;
  };
  if (background || isVisible()) {
    if (leading) tick();
    run();
  }
  const offVisibility = background
    ? () => {}
    : onVisibilityChange((visible) => {
        if (!visible) {
          pause();
          return;
        }
        reads.now();
        run();
      });
  if (onFocus) window.addEventListener("focus", tick);
  return {
    readNow: reads.now,
    stop: () => {
      reads.stop();
      pause();
      offVisibility();
      if (onFocus) window.removeEventListener("focus", tick);
    },
  };
}

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
  const latest = useRef<PollCallbacks<T>>({ read, onValue, onError, equal });
  useEffect(() => {
    latest.current = { read, onValue, onError, equal };
  });
  const now = useRef<() => void>(() => {});

  useEffect(() => {
    if (!enabled) return;
    const poll = startPoll(() => latest.current, { ms, serial, onFocus, background, leading });
    now.current = poll.readNow;
    return () => {
      now.current = () => {};
      poll.stop();
    };
  }, [ms, serial, onFocus, enabled, restartKey, background, leading]);

  return useCallback(() => {
    now.current();
  }, []);
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
