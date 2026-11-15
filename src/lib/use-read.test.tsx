// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useRead, type Read } from "@/lib/use-read";
import { flush } from "@/test/ui";

// A read is shown only for the key it was asked for: another key's answer, early or late, never
// shows, and a failure is kept as its reason.

/** A promise and the hands that settle it. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** `useRead` over two labs' reads, each answered when the test says, lab A's key shown first. */
function readTwoLabs() {
  const reads = { "lab-a": deferred<string>(), "lab-b": deferred<string>() };
  const { result, rerender } = renderHook((p: { key: "lab-a" | "lab-b" }) => useRead(p.key, () => reads[p.key].promise, "reading"), {
    initialProps: { key: "lab-a" as "lab-a" | "lab-b" },
  });
  return { reads, result, rerender };
}

describe("useRead", () => {
  it("is idle without a key, reading until the answer, then the answer", async () => {
    const answer = deferred<string>();
    const { result, rerender } = renderHook((p: { key: string | null }) => useRead(p.key, () => answer.promise, "reading"), {
      initialProps: { key: null as string | null },
    });
    expect(result.current).toEqual({ state: "idle" });
    rerender({ key: "lab-a" });
    expect(result.current).toEqual({ state: "reading" });
    await act(async () => {
      answer.resolve("brief a");
      await flush();
    });
    expect(result.current).toEqual({ state: "read", value: "brief a" });
  });

  it("never shows the previous key's answer, even when it arrives after the key changed", async () => {
    const { reads, result, rerender } = readTwoLabs();
    await act(async () => {
      reads["lab-a"].resolve("brief a");
      await flush();
    });
    rerender({ key: "lab-b" });
    // Lab A's answer is not lab B's: B reads as reading, not as A's brief.
    expect(result.current).toEqual({ state: "reading" });
    await act(async () => {
      reads["lab-b"].resolve("brief b");
      await flush();
    });
    expect(result.current).toEqual({ state: "read", value: "brief b" });
  });

  it("drops a late answer for a key no longer shown", async () => {
    const { reads, result, rerender } = readTwoLabs();
    rerender({ key: "lab-b" });
    await act(async () => {
      reads["lab-b"].resolve("brief b");
      await flush();
      reads["lab-a"].resolve("brief a");
      await flush();
    });
    expect(result.current).toEqual({ state: "read", value: "brief b" });
  });

  it("keeps a failure as its reason, never as a value", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { result } = renderHook(() => useRead<string>("lab-a", () => Promise.reject(new Error("offline")), "reading"));
    await act(flush);
    const read: Read<string> = result.current;
    expect(read.state).toBe("failed");
    expect(read.state === "failed" && read.error).toContain("offline");
  });
});
