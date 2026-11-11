// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useFlash } from "@/lib/use-flash";

describe("useFlash", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("is on for its time after a flash, then off again", () => {
    const { result } = renderHook(() => useFlash(1600));
    expect(result.current[0]).toBe(false);
    act(() => {
      result.current[1]();
    });
    expect(result.current[0]).toBe(true);
    act(() => {
      vi.advanceTimersByTime(1599);
    });
    expect(result.current[0]).toBe(true);
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(result.current[0]).toBe(false);
  });

  it("counts its time again from a second flash", () => {
    const { result } = renderHook(() => useFlash(1000));
    act(() => {
      result.current[1]();
      vi.advanceTimersByTime(800);
      result.current[1]();
      vi.advanceTimersByTime(800);
    });
    expect(result.current[0]).toBe(true);
    expect(vi.getTimerCount()).toBe(1);
  });

  // Regression: the "Copied" timer outlived its component, and fired after the test file's
  // environment was torn down (`ReferenceError: window is not defined`, settings-rows.test.tsx).
  it("leaves no timer behind once unmounted, and a late flash starts none", () => {
    const { result, unmount } = renderHook(() => useFlash(1600));
    act(() => {
      result.current[1]();
    });
    expect(vi.getTimerCount()).toBe(1);
    const flash = result.current[1];
    unmount();
    expect(vi.getTimerCount()).toBe(0);
    flash();
    expect(vi.getTimerCount()).toBe(0);
  });
});
