// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePoll, usePolled } from "@/lib/use-poll";
import { settle } from "@/test/ui";

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "visibilityState", { value: hidden ? "hidden" : "visible", configurable: true });
  document.dispatchEvent(new Event("visibilitychange"));
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  setHidden(false);
  vi.useRealTimers();
});

describe("usePoll", () => {
  it("is serial by default: a tick is skipped while a read is in flight", async () => {
    let resolve: (v: number) => void = () => {};
    const read = vi.fn(() => new Promise<number>((r) => (resolve = r)));
    renderHook(() => usePoll(read, 100, { onValue: vi.fn() }));
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(read).toHaveBeenCalledTimes(1);
    await settle(() => {
      resolve(1);
    });
    await act(() => vi.advanceTimersByTimeAsync(100));
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("overlaps reads when not serial", async () => {
    const read = vi.fn(() => new Promise<number>(() => {}));
    renderHook(() => usePoll(read, 100, { onValue: vi.fn(), serial: false }));
    await act(() => vi.advanceTimersByTimeAsync(300));
    expect(read).toHaveBeenCalledTimes(4);
  });

  it("delivers only the newest read started: an older one landing after it is dropped", async () => {
    const pending: ((v: string) => void)[] = [];
    const read = vi.fn(() => new Promise<string>((r) => pending.push(r)));
    const onValue = vi.fn();
    const { result } = renderHook(() => usePoll(read, 10_000, { onValue }));
    // Asked for while the first read is in flight: it starts anyway.
    act(() => {
      result.current();
    });
    expect(read).toHaveBeenCalledTimes(2);
    await settle(() => {
      pending[1]("new");
    });
    await settle(() => {
      pending[0]("old");
    });
    expect(onValue.mock.calls).toEqual([["new"]]);
  });

  it("skips an answer equal to the last one delivered when given `equal`", async () => {
    let n = 0;
    const read = vi.fn(() => Promise.resolve(n++ < 2 ? "same" : "other"));
    const onValue = vi.fn();
    renderHook(() => usePoll(read, 100, { onValue, equal: (a, b) => a === b }));
    await act(() => vi.advanceTimersByTimeAsync(250));
    expect(read).toHaveBeenCalledTimes(3);
    expect(onValue.mock.calls).toEqual([["same"], ["other"]]);
  });

  it("pauses while the window is hidden and reads at once when it shows", async () => {
    const read = vi.fn(() => Promise.resolve(1));
    renderHook(() => usePoll(read, 1000, { onValue: vi.fn() }));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(read).toHaveBeenCalledTimes(1);
    act(() => {
      setHidden(true);
    });
    await act(() => vi.advanceTimersByTimeAsync(10_000));
    expect(read).toHaveBeenCalledTimes(1);
    act(() => {
      setHidden(false);
    });
    expect(read).toHaveBeenCalledTimes(2);
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(read).toHaveBeenCalledTimes(3);
  });

  it("starts paused in a hidden window", async () => {
    setHidden(true);
    const read = vi.fn(() => Promise.resolve(1));
    renderHook(() => usePoll(read, 1000, { onValue: vi.fn() }));
    await act(() => vi.advanceTimersByTimeAsync(5000));
    expect(read).not.toHaveBeenCalled();
    act(() => {
      setHidden(false);
    });
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("goes on in a hidden window when `background`", async () => {
    setHidden(true);
    const read = vi.fn(() => Promise.resolve(1));
    renderHook(() => usePoll(read, 1000, { onValue: vi.fn(), background: true }));
    await act(() => vi.advanceTimersByTimeAsync(2000));
    expect(read).toHaveBeenCalledTimes(3);
  });

  it("waits for the first interval when not `leading`", async () => {
    const read = vi.fn(() => Promise.resolve(1));
    renderHook(() => usePoll(read, 1000, { onValue: vi.fn(), leading: false }));
    expect(read).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(read).toHaveBeenCalledTimes(1);
  });
});

describe("usePolled", () => {
  it("keeps the same object when an answer says the same thing", async () => {
    let n = 0;
    const read = vi.fn(() => Promise.resolve(new Set(n++ < 2 ? ["a"] : ["a", "b"])));
    const { result } = renderHook(() => usePolled(read, 1000, new Set<string>()));
    await act(() => vi.advanceTimersByTimeAsync(0));
    const first = result.current[0];
    expect([...first]).toEqual(["a"]);
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(result.current[0]).toBe(first);
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect([...result.current[0]]).toEqual(["a", "b"]);
  });

  it("takes a comparator of its own", async () => {
    let n = 0;
    const read = vi.fn(() => Promise.resolve({ n: n++ }));
    const { result } = renderHook(() => usePolled(read, 1000, { n: -1 }, { equal: () => false }));
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(result.current[0]).toEqual({ n: 1 });
  });
});
