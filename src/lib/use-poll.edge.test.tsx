// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { usePoll, usePolled } from "@/lib/use-poll";

type Deferred<T> = { promise: Promise<T>; resolve: (v: T) => void; reject: (e: unknown) => void };
function deferred<T>(): Deferred<T> {
  let resolve: (v: T) => void = () => {};
  let reject: (e: unknown) => void = () => {};
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

// What a poll does when its inputs change while a read is still in flight: the answer of a
// read that no longer belongs to the current poll must not land.
describe("usePoll: changes mid-flight", () => {
  afterEach(() => vi.useRealTimers());

  it("drops the answer of a read still in flight when the poll is disabled meanwhile", async () => {
    const d = deferred<number>();
    const read = vi.fn(() => d.promise);
    const onValue = vi.fn();
    const { rerender } = renderHook(({ on }) => usePoll(read, 1000, { onValue, enabled: on }), { initialProps: { on: true } });
    expect(read).toHaveBeenCalledTimes(1);
    rerender({ on: false });
    await act(async () => d.resolve(1));
    expect(onValue).not.toHaveBeenCalled();
  });

  it("restarts with an immediate read when restartKey changes, and drops the old key's late answer", async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    const read = vi.fn<() => Promise<string>>().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const onValue = vi.fn();
    const { rerender } = renderHook(({ k }) => usePoll(read, 1000, { onValue, restartKey: k }), { initialProps: { k: "a" } });
    rerender({ k: "b" });
    expect(read).toHaveBeenCalledTimes(2);
    await act(async () => first.resolve("old"));
    expect(onValue).not.toHaveBeenCalled();
    await act(async () => second.resolve("new"));
    expect(onValue).toHaveBeenCalledWith("new");
  });

  it("neither reports nor throws when a read rejects after unmount", async () => {
    const d = deferred<number>();
    const onError = vi.fn();
    const { unmount } = renderHook(() => usePoll(() => d.promise, 1000, { onValue: vi.fn(), onError }));
    unmount();
    // An unhandled rejection would fail this test on its own.
    await act(async () => d.reject(new Error("gone")));
    expect(onError).not.toHaveBeenCalled();
  });

  it("doesn't stack overlapping reads by default, and does when not serial", async () => {
    vi.useFakeTimers();
    const hung = () => new Promise<number>(() => {});
    const serial = vi.fn(hung);
    renderHook(() => usePoll(serial, 100, { onValue: vi.fn() }));
    const stacking = vi.fn(hung);
    renderHook(() => usePoll(stacking, 100, { onValue: vi.fn(), serial: false }));
    await act(async () => {
      vi.advanceTimersByTime(350);
    });
    expect(serial).toHaveBeenCalledTimes(1);
    expect(stacking).toHaveBeenCalledTimes(4);
  });

  it("uses the newest read function without restarting, and restarts with a read when the interval changes", async () => {
    vi.useFakeTimers();
    const a = vi.fn(async () => "a");
    const b = vi.fn(async () => "b");
    const onValue = vi.fn();
    const { rerender } = renderHook(({ read, ms }) => usePoll(read, ms, { onValue, serial: false }), { initialProps: { read: a, ms: 1000 } });
    rerender({ read: b, ms: 1000 });
    expect(b).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(b).toHaveBeenCalledTimes(1);
    rerender({ read: b, ms: 500 });
    expect(b).toHaveBeenCalledTimes(2);
  });

  it("stops listening for focus once disabled", () => {
    const read = vi.fn(async () => 1);
    const { rerender } = renderHook(({ on }) => usePoll(read, 1000, { onValue: vi.fn(), onFocus: true, enabled: on }), { initialProps: { on: true } });
    rerender({ on: false });
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("a serial poll reads again once a failed read settles", async () => {
    vi.useFakeTimers();
    const d = deferred<number>();
    const read = vi.fn<() => Promise<number>>().mockReturnValueOnce(d.promise).mockResolvedValue(2);
    renderHook(() => usePoll(read, 100, { onValue: vi.fn(), onError: vi.fn(), serial: true }));
    await act(async () => {
      vi.advanceTimersByTime(250);
    });
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => d.reject(new Error("x")));
    await act(async () => {
      vi.advanceTimersByTime(100);
    });
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("usePolled keeps the last answer across a restart rather than resetting to the initial", async () => {
    const { result, rerender } = renderHook(({ k }) => usePolled(async () => `v-${k}`, 10_000, "initial", { restartKey: k }), { initialProps: { k: 1 } });
    await act(async () => {});
    expect(result.current[0]).toBe("v-1");
    rerender({ k: 2 });
    expect(result.current[0]).toBe("v-1");
    await act(async () => {});
    expect(result.current[0]).toBe("v-2");
  });
});
