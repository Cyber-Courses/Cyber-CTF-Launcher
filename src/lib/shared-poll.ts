"use client";

import { useSyncExternalStore } from "react";
import { structurallyEqual } from "@/lib/equal";
import { ignore } from "@/lib/failure";
import { isVisible, onVisibilityChange } from "@/lib/visibility";
import { registerReset } from "@/lib/module-state";

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

/** The reading half of a shared poll: one read at a time, the next timed from the end of the
 *  previous one, publishing to `listeners` only what the newest read started says, and only
 *  when it changed. */
function pollReader<T>(o: SharedPollOptions<T>, listeners: ReadonlySet<() => void>) {
  const equal = o.equal ?? structurallyEqual;
  let value = o.initial;
  // Bumped on a reset: reads started before it neither publish nor count as in flight.
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let started = 0;
  let landed = 0;
  let inFlight = 0;

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
  const publish = (mine: number, gen: number, v: T) => {
    if (gen !== generation || mine < landed) return;
    landed = mine;
    if (equal(value, v)) return;
    value = v;
    for (const l of listeners) l();
  };
  function read() {
    clear();
    const mine = ++started;
    const gen = generation;
    inFlight++;
    o.read()
      .then(
        (v) => {
          publish(mine, gen, v);
        },
        (e: unknown) => {
          (o.onError ?? IGNORE)(e);
        },
      )
      .finally(() => {
        if (gen !== generation) return;
        inFlight--;
        schedule();
      });
  }
  // Back to its first value, nothing in flight: one test's answers must not reach the next.
  const reset = () => {
    generation++;
    clear();
    value = o.initial;
    inFlight = 0;
  };
  return { read, schedule, clear, reset, get: () => value };
}

/**
 * One poll of a backend command for the whole window, however many components read it: it
 * runs while at least one subscribes, one read at a time (the next read is timed from the end of
 * the previous one, so a slow read never stacks another), pauses while the window is hidden
 * (unless `hiddenMs` says otherwise) and reads at once when it shows again. Only the newest read
 * started may publish: a forced read (focus, `refresh`) overlapping the timer's must not have
 * the older answer land last.
 */
export function sharedPoll<T>(o: SharedPollOptions<T>): SharedPoll<T> {
  const listeners = new Set<() => void>();
  const reader = pollReader(o, listeners);
  let detach: () => void = () => {};
  const refresh = () => {
    if (listeners.size > 0) reader.read();
  };

  function start() {
    const offVisibility = onVisibilityChange((visible) => {
      if (visible) reader.read();
      else reader.schedule();
    });
    if (o.onFocus) window.addEventListener("focus", refresh);
    detach = () => {
      offVisibility();
      if (o.onFocus) window.removeEventListener("focus", refresh);
    };
    if (isVisible() || o.hiddenMs?.(reader.get()) != null) reader.read();
  }

  registerReset(reader.reset);

  return {
    subscribe(cb) {
      listeners.add(cb);
      if (listeners.size === 1) start();
      return () => {
        if (!listeners.delete(cb) || listeners.size > 0) return;
        reader.clear();
        detach();
        detach = () => {};
      };
    },
    get: reader.get,
    refresh,
    subscribers: () => listeners.size,
  };
}

const noSubscription = () => () => {};

/** A shared poll's latest answer, re-rendering only when it changes. Not `enabled` (a window
 *  that shows none of it, such as Settings), it doesn't subscribe, so it doesn't keep the poll going. */
export function useSharedPoll<T>(poll: SharedPoll<T>, enabled = true): T {
  return useSyncExternalStore(enabled ? poll.subscribe : noSubscription, poll.get, poll.get);
}
