// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useSidebarLabs } from "@/components/shell/use-sidebar-labs";
import type { ActiveOperation } from "@/lib/tauri";
import { commands, installTauri, type Call } from "@/test/tauri";
import { flush } from "@/test/ui";

const NONE = new Map<string, ActiveOperation>();
const ops = (...labIds: string[]) =>
  new Map(labIds.map((labId): [string, ActiveOperation] => [labId, { labId, op: "launch", machine: null, deploying: true, step: null, runtime: "DOCKER" }]));
const count = (calls: Call[], cmd: string) => commands(calls).filter((c) => c === cmd).length;

describe("useSidebarLabs", () => {
  it("lists a lab the running scan sees, from the window's one scan: no workload scan, no poll of the deploys of its own", async () => {
    let running = ["lab-invoice"];
    const calls = installTauri({ running_labs: () => running, active_operations: () => [], busy_labs: () => [] });
    const { result, rerender } = renderHook(({ inFlight }) => useSidebarLabs(true, inFlight), { initialProps: { inFlight: NONE } });
    await flush(50);
    expect(result.current.active.map((l) => l.id)).toEqual(["lab-invoice"]);
    expect(count(calls, "machine_workloads")).toBe(0);
    expect(count(calls, "busy_labs")).toBe(0);
    // An operation ends: the scan is read again at once, so a lab just stopped drops out.
    const before = count(calls, "running_labs");
    running = [];
    rerender({ inFlight: ops("lab-invoice") });
    rerender({ inFlight: NONE });
    await flush(50);
    expect(count(calls, "running_labs")).toBeGreaterThan(before);
    expect(result.current.active).toEqual([]);
  });

  it("reads nothing in a window that shows none of it", async () => {
    const calls = installTauri({ running_labs: () => ["lab-invoice"], active_operations: () => [] });
    renderHook(() => useSidebarLabs(true, NONE, false));
    await flush(50);
    expect(count(calls, "running_labs")).toBe(0);
    expect(count(calls, "active_operations")).toBe(0);
  });
});
