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
  /** One read at a time (the default): the next is timed from the end of the previous, so a slow
   *  read never stacks another. False times each read from the start of the previous one, a
   *  slow one overlapped. */
  serial?: boolean;
  /** Read at once when the poll starts (the default), or only after the first interval. */
  leading?: boolean;
}

export interface SharedPoll<T> {
  /** Starts the poll with the first subscriber, stops it with the last. */
  subscribe: (cb: () => void) => () => void;
  /** The latest answer (the same object until the answer changes). */
  get: () => T;
  /** Reads again right away, even with a read in flight (the timer starts over after it);
   *  resolves once that read has answered or failed. */
  refresh: () => Promise<void>;
  /** How many subscribe it now, for tests. */
  subscribers: () => number;
}

const IGNORE = ignore("polled again in a moment");

/** The reading half of a poll: one read at a time, the next timed from the end of the previous
 *  one (unless not `serial`), publishing to `listeners` only what the newest read started says,
 *  and only when it changed. */
function pollReader<T>(o: SharedPollOptions<T>, listeners: ReadonlySet<() => void>) {
  const equal = o.equal ?? structurallyEqual;
  const serial = o.serial ?? true;
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
  // The next read (after this one ends, when serial): now's visibility decides how soon, or
  // whether at all.
  const schedule = () => {
    clear();
    if (listeners.size === 0 || (serial && inFlight > 0)) return;
    const ms = isVisible() ? o.ms : (o.hiddenMs?.(value) ?? null);
    if (ms !== null)
      timer = setTimeout(() => {
        void read();
      }, ms);
  };
  const publish = (mine: number, gen: number, v: T) => {
    if (gen !== generation || mine < landed) return;
    landed = mine;
    if (equal(value, v)) return;
    value = v;
    for (const l of listeners) l();
  };
  function read(): Promise<void> {
    clear();
    const mine = ++started;
    const gen = generation;
    inFlight++;
    if (!serial) schedule();
    return o
      .read()
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
        if (serial) schedule();
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
 * A poll of a backend command, the one engine behind every poll of the app: it runs while at
 * least one listener subscribes, one read at a time (unless not `serial`: the next read is timed
 * from the end of the previous one, so a slow read never stacks another), pauses while the window
 * is hidden (unless `hiddenMs` says otherwise) and reads at once when it shows again. Only the
 * newest read started may publish: a forced read (focus, `refresh`) overlapping the timer's must
 * not have the older answer land last. `sharedPoll` makes one for a whole window; `usePoll`, one
 * for a component.
 */
export function createPoll<T>(o: SharedPollOptions<T>): SharedPoll<T> & { reset: () => void } {
  const listeners = new Set<() => void>();
  const reader = pollReader(o, listeners);
  let detach: () => void = () => {};
  const refresh = () => (listeners.size > 0 ? reader.read() : Promise.resolve());
  const onFocus = () => {
    void refresh();
  };

  function start() {
    const offVisibility = onVisibilityChange((visible) => {
      if (visible) void reader.read();
      else reader.schedule();
    });
    if (o.onFocus) window.addEventListener("focus", onFocus);
    detach = () => {
      offVisibility();
      if (o.onFocus) window.removeEventListener("focus", onFocus);
    };
    if (!isVisible() && o.hiddenMs?.(reader.get()) == null) return;
    if (o.leading ?? true) void reader.read();
    else reader.schedule();
  }

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
    reset: reader.reset,
  };
}

/** One poll of a backend command for the whole window, however many components read it (see
 *  `createPoll`), back to its first value between tests. */
export function sharedPoll<T>(o: SharedPollOptions<T>): SharedPoll<T> {
  const poll = createPoll(o);
  registerReset(poll.reset);
  return poll;
}

const noSubscription = () => () => {};

/** A shared poll's latest answer, re-rendering only when it changes. Not `enabled` (a window
 *  that shows none of it, such as Settings), it doesn't subscribe, so it doesn't keep the poll going. */
export function useSharedPoll<T>(poll: SharedPoll<T>, enabled = true): T {
  return useSyncExternalStore(enabled ? poll.subscribe : noSubscription, poll.get, poll.get);
}
