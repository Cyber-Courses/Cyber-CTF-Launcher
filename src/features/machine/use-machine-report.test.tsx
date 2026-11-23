// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useMachineReport } from "@/features/machine/use-machine-report";
import { readyReport } from "@/test/fixtures";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

describe("useMachineReport", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("recovers on its own from a check that failed before any answered, and logs the failure once", async () => {
    // Regression: the machine-setup window stopped checking after a first failure (Try again
    // only), and its own copy of this hook, unlike the onboarding's, never logged a failure.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    let failures = 2;
    installTauri({
      system_check: () => {
        if (failures-- > 0) throw new Error("hung");
        return readyReport();
      },
    });
    const { result } = renderHook(() => useMachineReport());
    await flush();
    expect(result.current.checkError).toContain("hung");
    await act(() => vi.advanceTimersByTimeAsync(15_000));
    await flush();
    expect(failures).toBeLessThan(0);
    expect(result.current.report).not.toBeNull();
    expect(result.current.checkError).toBeNull();
    expect(warned.mock.calls.filter(([what]) => what === "system check:")).toHaveLength(1);
  });
});
