// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { getDeploySnapshot } from "@/features/labs/runs/deploy-runs";
import { testLab } from "@/features/labs/labs-test-kit";
import { installTauri } from "@/test/tauri";

// An operation stays busy until the lab's status after it is read, so a status read that never
// answers would otherwise pin the lab's buttons for good: the wait is capped at 20 s.

const lab = testLab("lab-hung", { runtime: { runtime: "DOCKER", architectures: ["x86_64"], providers: [] } });

/** Advances fake time and lets the promises it releases settle. */
const advance = (ms: number) =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });

afterEach(() => {
  vi.useRealTimers();
});

describe("useLabActions: the status read after an operation", () => {
  it("lets the lab settle after 20 s when the read never answers", async () => {
    vi.useFakeTimers();
    installTauri({ busy_labs: () => [], active_operations: () => [] });
    // A refresh that hangs: the backend never answers the status read.
    const refresh = vi.fn(() => new Promise<void>(() => {}));
    const { result } = renderHook(() => useLabActions(refresh, []));
    let launched: Promise<void> | undefined;
    act(() => {
      launched = result.current.launch(lab, null, undefined, { ports: "default" });
    });
    await advance(0);
    expect(refresh).toHaveBeenCalledWith(lab, { fresh: true });
    // The launch itself went through; the run waits on the read.
    expect(getDeploySnapshot().runs[lab.id]?.logs).toContain("✓ Lab is running");
    expect(getDeploySnapshot().runs[lab.id]?.busy).toBe(true);
    await advance(19_999);
    expect(getDeploySnapshot().runs[lab.id]?.busy).toBe(true);
    await advance(1);
    expect(getDeploySnapshot().runs[lab.id]?.busy).toBe(false);
    await act(async () => {
      await launched;
    });
  });
});
