// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { sharedPoll, useSharedPoll } from "@/lib/shared-poll";
import { structurallyEqual } from "@/lib/equal";

/** Puts the window on or off screen, as the browser does. */
function setHidden(hidden: boolean) {
  Object.defineProperty(document, "visibilityState", { value: hidden ? "hidden" : "visible", configurable: true });
  document.dispatchEvent(new Event("visibilitychange"));
}

/** A read whose answers the test hands out one by one, in any order. */
function deferredReads<T>() {
  const pending: { resolve: (v: T) => void; reject: (e: unknown) => void }[] = [];
  const read = vi.fn(() => new Promise<T>((resolve, reject) => pending.push({ resolve, reject })));
  return { read, pending };
}

const settle = () => act(() => vi.advanceTimersByTimeAsync(0));

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  setHidden(false);
  vi.useRealTimers();
});

describe("sharedPoll", () => {
  it("starts with the first subscriber, is shared by the rest, and stops with the last", async () => {
    const read = vi.fn(async () => [1]);
    const poll = sharedPoll({ read, ms: 1000, initial: [] as number[] });
    // Nobody reads it: nothing is polled.
    await vi.advanceTimersByTimeAsync(5000);
    expect(read).not.toHaveBeenCalled();

    const a = vi.fn();
    const b = vi.fn();
    const offA = poll.subscribe(a);
    const offB = poll.subscribe(b);
    expect(poll.subscribers()).toBe(2);
    // One read for both, at once.
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(poll.get()).toEqual([1]);
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
    // Then one read per interval, whatever the number of subscribers.
    await vi.advanceTimersByTimeAsync(3000);
    expect(read).toHaveBeenCalledTimes(4);

    offA();
    await vi.advanceTimersByTimeAsync(1000);
    expect(read).toHaveBeenCalledTimes(5);
    offB();
    // Unsubscribing twice is harmless.
    offB();
    expect(poll.subscribers()).toBe(0);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(read).toHaveBeenCalledTimes(5);
    // A refresh with nobody listening reads nothing.
    poll.refresh();
    expect(read).toHaveBeenCalledTimes(5);

    // Subscribed again: it starts over, keeping the last answer meanwhile.
    const offC = poll.subscribe(() => {});
    expect(read).toHaveBeenCalledTimes(6);
    expect(poll.get()).toEqual([1]);
    offC();
  });

  it("publishes an answer only when it changes, keeping the same object", async () => {
    let n = 0;
    const read = vi.fn(async () => new Set(n++ < 2 ? ["a"] : ["a", "b"]));
    const poll = sharedPoll({ read, ms: 1000, initial: new Set<string>() });
    const cb = vi.fn();
    const off = poll.subscribe(cb);
    await vi.advanceTimersByTimeAsync(0);
    const first = poll.get();
    expect([...first]).toEqual(["a"]);
    expect(cb).toHaveBeenCalledTimes(1);
    // The same set again: not published, the same object kept.
    await vi.advanceTimersByTimeAsync(1000);
    expect(read).toHaveBeenCalledTimes(2);
    expect(cb).toHaveBeenCalledTimes(1);
    expect(poll.get()).toBe(first);
    // A different one is.
    await vi.advanceTimersByTimeAsync(1000);
    expect([...poll.get()]).toEqual(["a", "b"]);
    expect(cb).toHaveBeenCalledTimes(2);
    off();
  });

  it("takes a comparator of its own", async () => {
    const read = vi.fn(async () => ({ at: Date.now(), value: 1 }));
    const poll = sharedPoll({ read, ms: 1000, initial: { at: 0, value: 0 }, equal: (a, b) => a.value === b.value });
    const cb = vi.fn();
    const off = poll.subscribe(cb);
    await vi.advanceTimersByTimeAsync(3000);
    // `at` changes every read; only `value` counts.
    expect(read).toHaveBeenCalledTimes(4);
    expect(cb).toHaveBeenCalledTimes(1);
    off();
  });

  it("reads one at a time: the next read waits for a slow one to end", async () => {
    const { read, pending } = deferredReads<number>();
    const poll = sharedPoll({ read, ms: 1000, initial: 0 });
    const off = poll.subscribe(() => {});
    expect(read).toHaveBeenCalledTimes(1);
    // Wedged for 10 intervals: no read stacks on top of it.
    await vi.advanceTimersByTimeAsync(10_000);
    expect(read).toHaveBeenCalledTimes(1);
    pending[0].resolve(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(poll.get()).toBe(1);
    // Timed from its end.
    await vi.advanceTimersByTimeAsync(999);
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(read).toHaveBeenCalledTimes(2);
    off();
  });

  it("lets only the newest read started publish, whatever order they answer in", async () => {
    const { read, pending } = deferredReads<string>();
    const poll = sharedPoll({ read, ms: 1000, initial: "none" });
    const off = poll.subscribe(() => {});
    // A forced read while the timer's is in flight.
    poll.refresh();
    expect(read).toHaveBeenCalledTimes(2);
    pending[1].resolve("new");
    await vi.advanceTimersByTimeAsync(0);
    expect(poll.get()).toBe("new");
    // The older read lands last with older news: dropped.
    pending[0].resolve("old");
    await vi.advanceTimersByTimeAsync(0);
    expect(poll.get()).toBe("new");
    // In order, both count.
    await vi.advanceTimersByTimeAsync(1000);
    poll.refresh();
    pending[2].resolve("third");
    await vi.advanceTimersByTimeAsync(0);
    expect(poll.get()).toBe("third");
    pending[3].resolve("fourth");
    await vi.advanceTimersByTimeAsync(0);
    expect(poll.get()).toBe("fourth");
    off();
  });

  it("pauses while the window is hidden and reads at once when it shows", async () => {
    const read = vi.fn(async () => 1);
    const poll = sharedPoll({ read, ms: 1000, initial: 0 });
    const off = poll.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(read).toHaveBeenCalledTimes(1);
    setHidden(true);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(read).toHaveBeenCalledTimes(1);
    setHidden(false);
    expect(read).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1000);
    expect(read).toHaveBeenCalledTimes(3);
    off();
  });

  it("starts paused in a hidden window, and stops listening once nobody reads it", async () => {
    setHidden(true);
    const read = vi.fn(async () => 1);
    const poll = sharedPoll({ read, ms: 1000, initial: 0 });
    const off = poll.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(5000);
    expect(read).not.toHaveBeenCalled();
    off();
    setHidden(false);
    expect(read).not.toHaveBeenCalled();
  });

  it("goes on in a hidden window at the pace `hiddenMs` gives for the current answer", async () => {
    let busy = false;
    const read = vi.fn(async () => (busy ? ["op"] : []));
    const poll = sharedPoll({ read, ms: 1000, initial: [] as string[], hiddenMs: (v) => (v.length > 0 ? 1000 : 10_000) });
    setHidden(true);
    // Hidden from the start, it still reads.
    const off = poll.subscribe(() => {});
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(9_999);
    expect(read).toHaveBeenCalledTimes(1);
    busy = true;
    await vi.advanceTimersByTimeAsync(1);
    expect(read).toHaveBeenCalledTimes(2);
    // Busy now: the faster pace.
    await vi.advanceTimersByTimeAsync(1000);
    expect(read).toHaveBeenCalledTimes(3);
    off();
  });

  it("reads on focus when asked, and hands failures to onError", async () => {
    const onError = vi.fn();
    const read = vi.fn(async () => {
      throw new Error("down");
    });
    const poll = sharedPoll({ read, ms: 60_000, initial: 0, onFocus: true, onError });
    const off = poll.subscribe(() => {});
    window.dispatchEvent(new Event("focus"));
    expect(read).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(0);
    expect(onError).toHaveBeenCalledTimes(2);
    expect(poll.get()).toBe(0);
    off();
    window.dispatchEvent(new Event("focus"));
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("ignores failures by default", async () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    const poll = sharedPoll({ read: () => Promise.reject(new Error("down")), ms: 1000, initial: 0 });
    const off = poll.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(poll.get()).toBe(0);
    off();
    debug.mockRestore();
  });

  it("feeds a hook that re-renders only on a change", async () => {
    let value = 1;
    const poll = sharedPoll({ read: async () => ({ value }), ms: 1000, initial: { value: 0 } });
    let renders = 0;
    const { result, unmount } = renderHook(() => {
      renders++;
      return useSharedPoll(poll);
    });
    await settle();
    expect(result.current).toEqual({ value: 1 });
    const seen = renders;
    await act(() => vi.advanceTimersByTimeAsync(3000));
    expect(renders).toBe(seen);
    value = 2;
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(result.current).toEqual({ value: 2 });
    unmount();
    expect(poll.subscribers()).toBe(0);
  });
});

describe("structurallyEqual", () => {
  it("compares by content", () => {
    expect(
      structurallyEqual(
        { a: [1, { b: "x" }], s: new Set([1]), m: new Map([["k", { v: 1 }]]) },
        { a: [1, { b: "x" }], s: new Set([1]), m: new Map([["k", { v: 1 }]]) },
      ),
    ).toBe(true);
    expect(structurallyEqual(NaN, NaN)).toBe(true);
    expect(structurallyEqual({ a: 1 }, { a: 2 })).toBe(false);
    expect(structurallyEqual({ a: 1 }, { b: 1 })).toBe(false);
    expect(structurallyEqual({ a: 1 }, { a: 1, b: 2 })).toBe(false);
    expect(structurallyEqual([1, 2], [1])).toBe(false);
    expect(structurallyEqual([1], { 0: 1 })).toBe(false);
    expect(structurallyEqual(new Set([1]), new Set([2]))).toBe(false);
    expect(structurallyEqual(new Set([1]), new Set([1, 2]))).toBe(false);
    expect(structurallyEqual(new Set([1]), [1])).toBe(false);
    expect(structurallyEqual(new Map([["k", 1]]), new Map([["k", 2]]))).toBe(false);
    expect(structurallyEqual(new Map([["k", 1]]), new Map([["j", 1]]))).toBe(false);
    expect(structurallyEqual(new Map([["k", 1]]), new Map())).toBe(false);
    expect(structurallyEqual(new Map(), {})).toBe(false);
    expect(structurallyEqual(null, {})).toBe(false);
    expect(structurallyEqual(undefined, null)).toBe(false);
    expect(structurallyEqual(1, "1")).toBe(false);
  });
});
