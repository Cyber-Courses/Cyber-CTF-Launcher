"use client";

import { useSyncExternalStore } from "react";
import { structurallyEqual } from "@/lib/equal";
import { ignore } from "@/lib/failure";
import { isVisible, onVisibilityChange } from "@/lib/visibility";

export interface SharedPollOptions<T> {
  read: () => Promise<T>;
  /** The interval between the end of one read and the start of the next, while visible. */
  ms: number;
  /** The answer until the first read lands. */
  initial: T;
  /** Whether two answers say the same thing (default: structurally equal). An answer equal to
   *  the current one is not published, so nothing reading it re-renders. */
  equal?: (a: T, b: T) => boolean;
  /** Also read when the window regains focus. */
  onFocus?: boolean;
  /** While the window is hidden, how long to wait between reads given the current answer; null
   *  (or no function) pauses until the window shows again. For what must go on in the
   *  background. */
  hiddenMs?: (value: T) => number | null;
  /** A failed read; by default ignored, since the next one reads again. */
  onError?: (e: unknown) => void;
}

export interface SharedPoll<T> {
  /** Starts the poll with the first subscriber, stops it with the last. */
  subscribe: (cb: () => void) => () => void;
  /** The latest answer (the same object until the answer changes). */
  get: () => T;
  /** Reads again right away (the timer starts over after it). */
  refresh: () => void;
  /** How many subscribe it now, for tests. */
  subscribers: () => number;
}

const IGNORE = ignore("polled again in a moment");

/**
 * One poll of a backend command for the whole window, however many components read it: it
 * runs while at least one subscribes, one read at a time (the next read is timed from the end of
 * the previous one, so a slow read never stacks another), pauses while the window is hidden
 * (unless `hiddenMs` says otherwise) and reads at once when it shows again. Only the newest read
 * started may publish: a forced read (focus, `refresh`) overlapping the timer's must not have
 * the older answer land last.
 */
export function sharedPoll<T>(o: SharedPollOptions<T>): SharedPoll<T> {
  const equal = o.equal ?? structurallyEqual;
  const listeners = new Set<() => void>();
  let value = o.initial;
  // Bumped by `resetSharedPolls`: reads started before it neither publish nor count as in flight.
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let started = 0;
  let landed = 0;
  let inFlight = 0;
  let detach: () => void = () => {};

  const clear = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
  // The next read after this one ends: now's visibility decides how soon, or whether at all.
  const schedule = () => {
    clear();
    if (listeners.size === 0 || inFlight > 0) return;
    const ms = isVisible() ? o.ms : (o.hiddenMs?.(value) ?? null);
    if (ms !== null) timer = setTimeout(read, ms);
  };
  function read() {
    clear();
    const mine = ++started;
    const gen = generation;
    inFlight++;
    o.read()
      .then(
        (v) => {
          if (gen !== generation || mine < landed) return;
          landed = mine;
          if (equal(value, v)) return;
          value = v;
          for (const l of listeners) l();
        },
        (e) => (o.onError ?? IGNORE)(e),
      )
      .finally(() => {
        if (gen !== generation) return;
        inFlight--;
        schedule();
      });
  }
  const refresh = () => {
    if (listeners.size > 0) read();
  };

  function start() {
    const offVisibility = onVisibilityChange((visible) => (visible ? read() : schedule()));
    if (o.onFocus) window.addEventListener("focus", refresh);
    detach = () => {
      offVisibility();
      if (o.onFocus) window.removeEventListener("focus", refresh);
    };
    if (isVisible() || o.hiddenMs?.(value) != null) read();
  }

  resets.add(() => {
    generation++;
    clear();
    value = o.initial;
    inFlight = 0;
  });

  return {
    subscribe(cb) {
      listeners.add(cb);
      if (listeners.size === 1) start();
      return () => {
        if (!listeners.delete(cb) || listeners.size > 0) return;
        clear();
        detach();
        detach = () => {};
      };
    },
    get: () => value,
    refresh,
    subscribers: () => listeners.size,
  };
}

const noSubscription = () => () => {};

/** Every shared poll's reset, for `resetSharedPolls`. */
const resets = new Set<() => void>();

/** Puts every shared poll back to its first value, nothing in flight (tests start from a clean
 *  window: the polls live at module level, so one test's answers otherwise reach the next). */
export function resetSharedPolls() {
  for (const reset of resets) reset();
}

/** A shared poll's latest answer, re-rendering only when it changes. Not `enabled` (a window
 *  that shows none of it, such as Settings), it doesn't subscribe, so it doesn't keep the poll going. */
export function useSharedPoll<T>(poll: SharedPoll<T>, enabled = true): T {
  return useSyncExternalStore(enabled ? poll.subscribe : noSubscription, poll.get, poll.get);
}
