// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useAction } from "@/lib/use-action";
import { flush } from "@/test/ui";

// An action runs once at a time: a second start while one is under way does nothing, its state
// is one value (idle, running, failed), and a failure releases the guard for a retry.

/** A promise and the hands that settle it. */
function deferred() {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useAction", () => {
  it("starts once when run twice in one frame, and again once the first has ended", async () => {
    let pending = deferred();
    const act_ = vi.fn(() => pending.promise);
    const { result } = renderHook(() => useAction(act_));
    act(() => {
      result.current.run();
      result.current.run();
    });
    expect(act_).toHaveBeenCalledTimes(1);
    expect(result.current.state).toEqual({ state: "running" });
    await act(async () => {
      pending.resolve();
      await flush();
    });
    expect(result.current.state).toEqual({ state: "idle" });
    pending = deferred();
    act(() => {
      result.current.run();
    });
    expect(act_).toHaveBeenCalledTimes(2);
  });

  it("keeps a failure as its reason and lets a retry start, which clears it", async () => {
    const first = deferred();
    const second = deferred();
    const act_ = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useAction(act_));
    act(() => {
      result.current.run();
    });
    await act(async () => {
      first.reject(new Error("refused"));
      await flush();
    });
    expect(result.current.state.state).toBe("failed");
    expect(result.current.error).toContain("refused");
    expect(result.current.running).toBe(false);
    act(() => {
      result.current.run();
    });
    expect(act_).toHaveBeenCalledTimes(2);
    expect(result.current.error).toBeNull();
    expect(result.current.running).toBe(true);
  });

  it("passes the run's arguments and uses the latest action", async () => {
    const first = vi.fn<(id: string) => Promise<void>>(() => Promise.resolve());
    const second = vi.fn<(id: string) => Promise<void>>(() => Promise.resolve());
    const { result, rerender } = renderHook((p: { a: (id: string) => Promise<void> }) => useAction(p.a), { initialProps: { a: first } });
    rerender({ a: second });
    act(() => {
      result.current.run("lab-sqli");
    });
    await act(flush);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith("lab-sqli");
  });
});
