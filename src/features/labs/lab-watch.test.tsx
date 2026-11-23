// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Lab } from "@/features/labs/use-labs";
import { useScannedRunning, useWatchLab, useWatchLabs } from "@/features/labs/lab-watch";
import { getLabState } from "@/features/labs/lab-store";
import { tellStatus } from "@/lib/dev-mock/run-service";
import type { LabStatus } from "@/lib/tauri";
import { commands, installTauri, rejectWith, type Call } from "@/test/tauri";
import { flush } from "@/test/ui";

// The labs' statuses come from the core's reconcile, never from a poll: the window says what it
// watches (and which lab pages are open), the core tells each status, and what changed after.

const lab = (id: string, runtime = "DOCKER") => ({ id, slug: id, title: id, runtime: { runtime, architectures: [], providers: [] } }) as unknown as Lab;
const status = (running: boolean) => ({ running, parked: null, machines: [], networks: [], url: null }) as unknown as LabStatus;
const watches = (calls: Call[]) => calls.filter((c) => c.cmd === "labs_watch").map((c) => c.args);

describe("watching the labs", () => {
  it("tells the core the labs shown and the pages open, and takes in the statuses it tells", async () => {
    const calls = installTauri({ lab_status: (a) => status(a?.id === "a"), running_labs: () => ["a"] });
    const labs = [lab("a"), lab("b", "VM")];
    renderHook(() => {
      useWatchLabs(labs);
      useWatchLab("b");
    });
    await flush();
    expect(watches(calls).at(-1)).toEqual({
      labs: [
        { id: "a", runtime: "DOCKER" },
        { id: "b", runtime: "VM" },
      ],
      open: ["b"],
    });
    expect(getLabState().statuses.a.running).toBe(true);
    expect(getLabState().statuses.b.running).toBe(false);
    expect([...getLabState().scanRunning]).toEqual(["a"]);
    expect([...getLabState().probed].sort()).toEqual(["a", "b"]);
    // A change the core tells later lands without any read of this window's.
    const before = commands(calls).length;
    act(() => {
      tellStatus("b", status(true));
    });
    expect(getLabState().statuses.b.running).toBe(true);
    expect(commands(calls).slice(before)).toEqual([]);
  });

  it("never polls: no status read or scan happens with time alone", async () => {
    const calls = installTauri({ lab_status: () => status(false), running_labs: () => [] });
    renderHook(() => {
      useWatchLabs([lab("a")]);
    });
    await flush();
    const after = commands(calls).length;
    vi.useFakeTimers();
    await act(() => vi.advanceTimersByTimeAsync(60_000));
    vi.useRealTimers();
    expect(commands(calls).slice(after)).toEqual([]);
  });

  it("tells the same watch once, and a change of what is watched again", async () => {
    const calls = installTauri({ lab_status: () => status(false), running_labs: () => [] });
    const one = [lab("a")];
    const { rerender, unmount } = renderHook(
      ({ labs }) => {
        useWatchLabs(labs);
      },
      { initialProps: { labs: one } },
    );
    await flush();
    rerender({ labs: one });
    await flush();
    expect(watches(calls)).toHaveLength(1);
    rerender({ labs: [lab("a"), lab("c")] });
    await flush();
    expect(watches(calls)).toHaveLength(2);
    unmount();
    await flush();
    expect(watches(calls).at(-1)).toEqual({ labs: [], open: [] });
  });

  it("keeps a failed read's error for the lab's page", async () => {
    installTauri({
      lab_status: () => {
        rejectWith({ code: "dockerNotRunning", message: "Docker isn't running." });
      },
      running_labs: () => [],
    });
    renderHook(() => {
      useWatchLabs([lab("a")]);
    });
    await flush();
    expect(getLabState().statusErrors.a).toMatchObject({ code: "dockerNotRunning" });
  });

  it("gives the sidebar what the core's scan sees up", async () => {
    installTauri({ running_labs: () => ["x", "y"], lab_status: () => status(false) });
    const scanned = renderHook(() => useScannedRunning(true));
    renderHook(() => {
      useWatchLabs([lab("x")]);
    });
    await flush();
    expect([...scanned.result.current].sort()).toEqual(["x", "y"]);
  });
});
