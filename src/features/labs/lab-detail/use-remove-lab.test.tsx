// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useRemoveLab } from "@/features/labs/lab-detail/use-remove-lab";
import { testLab } from "@/features/labs/labs-test-kit";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// Remove lab runs once at a time: started again while its removal runs, it asks nothing more and
// removes nothing more.

describe("useRemoveLab", () => {
  it("asks once and removes once when started again while the removal runs", async () => {
    let finish: (v: unknown) => void = () => undefined;
    const calls = installTauri({
      lab_installed: () => true,
      lab_remove: () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    });
    const { result } = renderHook(() => useRemoveLab(testLab("lab-sqli"), "", vi.fn()));
    await act(flush);
    act(() => {
      result.current.remove();
    });
    await act(flush);
    expect(result.current.removing).toBe(true);
    act(() => {
      result.current.remove();
    });
    await act(flush);
    expect(calls.filter((c) => c.cmd === "plugin:dialog|message")).toHaveLength(1);
    expect(calls.filter((c) => c.cmd === "lab_remove")).toHaveLength(1);
    await act(async () => {
      finish({ leftovers: [] });
      await flush();
    });
  });
});
