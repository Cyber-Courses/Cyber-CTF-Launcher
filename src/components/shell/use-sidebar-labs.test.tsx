// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useSidebarLabs } from "@/components/shell/use-sidebar-labs";
import type { ActiveOperation } from "@/lib/tauri";
import { tellRunning } from "@/lib/dev-mock/run-service";
import { commands, installTauri, type Call } from "@/test/tauri";
import { flush } from "@/test/ui";

const NONE = new Map<string, ActiveOperation>();
const ops = (...labIds: string[]) =>
  new Map(labIds.map((labId): [string, ActiveOperation] => [labId, { labId, op: "launch", machine: null, deploying: true, step: null, runtime: "DOCKER" }]));
const count = (calls: Call[], cmd: string) => commands(calls).filter((c) => c === cmd).length;

describe("useSidebarLabs", () => {
  it("lists a lab the core's scan sees, from its events: no workload scan, no poll of the deploys of its own", async () => {
    const calls = installTauri({ running_labs: () => ["lab-invoice"], active_operations: () => [], busy_labs: () => [] });
    const { result, rerender } = renderHook(({ inFlight }) => useSidebarLabs(true, inFlight), { initialProps: { inFlight: NONE } });
    await flush(50);
    expect(result.current.active.map((l) => l.id)).toEqual(["lab-invoice"]);
    expect(count(calls, "machine_workloads")).toBe(0);
    expect(count(calls, "busy_labs")).toBe(0);
    // The lab stops: the core tells its scan changed, and the lab drops out.
    rerender({ inFlight: ops("lab-invoice") });
    act(() => {
      tellRunning([]);
    });
    rerender({ inFlight: NONE });
    await flush(50);
    expect(result.current.active).toEqual([]);
  });

  it("reads nothing in a window that shows none of it", async () => {
    const calls = installTauri({ running_labs: () => ["lab-invoice"], active_operations: () => [] });
    renderHook(() => useSidebarLabs(true, NONE, false));
    await flush(50);
    expect(count(calls, "run_events")).toBe(0);
  });
});
