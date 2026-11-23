// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { usePoll, usePolled } from "@/lib/use-poll";
import { settle } from "@/test/ui";
import { deferred } from "@/test/deferred";

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
    await settle(() => {
      d.resolve(1);
    });
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
    await settle(() => {
      first.resolve("old");
    });
    expect(onValue).not.toHaveBeenCalled();
    await settle(() => {
      second.resolve("new");
    });
    expect(onValue).toHaveBeenCalledWith("new");
  });

  it("neither reports nor throws when a read rejects after unmount", async () => {
    const d = deferred<number>();
    const onError = vi.fn();
    const { unmount } = renderHook(() => usePoll(() => d.promise, 1000, { onValue: vi.fn(), onError }));
    unmount();
    // An unhandled rejection would fail this test on its own.
    await settle(() => {
      d.reject(new Error("gone"));
    });
    expect(onError).not.toHaveBeenCalled();
  });

  it("doesn't stack overlapping reads by default, and does when not serial", async () => {
    vi.useFakeTimers();
    const hung = () => new Promise<number>(() => {});
    const serial = vi.fn(hung);
    renderHook(() => usePoll(serial, 100, { onValue: vi.fn() }));
    const stacking = vi.fn(hung);
    renderHook(() => usePoll(stacking, 100, { onValue: vi.fn(), serial: false }));
    await settle(() => {
      vi.advanceTimersByTime(350);
    });
    expect(serial).toHaveBeenCalledTimes(1);
    expect(stacking).toHaveBeenCalledTimes(4);
  });

  it("uses the newest read function without restarting, and restarts with a read when the interval changes", async () => {
    vi.useFakeTimers();
    const a = vi.fn(() => Promise.resolve("a"));
    const b = vi.fn(() => Promise.resolve("b"));
    const onValue = vi.fn();
    const { rerender } = renderHook(({ read, ms }) => usePoll(read, ms, { onValue, serial: false }), { initialProps: { read: a, ms: 1000 } });
    rerender({ read: b, ms: 1000 });
    expect(b).not.toHaveBeenCalled();
    await settle(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(b).toHaveBeenCalledTimes(1);
    rerender({ read: b, ms: 500 });
    expect(b).toHaveBeenCalledTimes(2);
  });

  it("stops listening for focus once disabled", () => {
    const read = vi.fn(() => Promise.resolve(1));
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
    await settle(() => {
      vi.advanceTimersByTime(250);
    });
    expect(read).toHaveBeenCalledTimes(1);
    await settle(() => {
      d.reject(new Error("x"));
    });
    await settle(() => {
      vi.advanceTimersByTime(100);
    });
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("usePolled keeps the last answer across a restart rather than resetting to the initial", async () => {
    const { result, rerender } = renderHook(({ k }) => usePolled(() => Promise.resolve(`v-${String(k)}`), 10_000, "initial", { restartKey: k }), {
      initialProps: { k: 1 },
    });
    await act(async () => {});
    expect(result.current[0]).toBe("v-1");
    rerender({ k: 2 });
    expect(result.current[0]).toBe("v-1");
    await act(async () => {});
    expect(result.current[0]).toBe("v-2");
  });
});
