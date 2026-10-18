// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useLatest } from "@/lib/use-latest";

describe("useLatest", () => {
  it("keeps only the newest request of a key, each key on its own", () => {
    const { result } = renderHook(() => useLatest());
    const first = result.current();
    const second = result.current();
    expect(first()).toBe(false);
    expect(second()).toBe(true);
    const a = result.current("a");
    const b = result.current("b");
    expect(a()).toBe(true);
    expect(b()).toBe(true);
    expect(second()).toBe(true);
  });

  it("drops every answer once the component is gone", () => {
    const { result, unmount } = renderHook(() => useLatest());
    const pending = result.current();
    unmount();
    expect(pending()).toBe(false);
  });

  it("is the same function from render to render", () => {
    const { result, rerender } = renderHook(() => useLatest());
    const begin = result.current;
    rerender();
    expect(result.current).toBe(begin);
  });
});
