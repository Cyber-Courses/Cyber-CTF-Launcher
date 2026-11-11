// @vitest-environment jsdom
import { render, renderHook, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UsageStats } from "@/features/home/home-sections";
import { StatCard } from "@/features/machine/machine-parts";
import { useMachineMetrics } from "@/features/machine/use-machine-metrics";
import { installTauri, rejects } from "@/test/tauri";
import { flush } from "@/test/ui";

// The machine's usage figures when they can't be read: the hook says the read failed, and the
// stat cards show a dash instead of placeholders that never fill.

describe("useMachineMetrics", () => {
  it("reports a failed read, with no metrics and an empty history", async () => {
    installTauri({ machine_metrics: rejects({ code: "other", message: "sysinfo unavailable" }) });
    const said = vi.spyOn(console, "debug").mockImplementation(() => {});
    const { result } = renderHook(() => useMachineMetrics(60_000));
    await flush();
    expect(result.current.failed).toBe(true);
    // Said in the log, as every quiet failure is, not swallowed.
    expect(said).toHaveBeenCalledWith(expect.stringContaining("usage figures"), expect.objectContaining({ message: "sysinfo unavailable" }));
    said.mockRestore();
    expect(result.current.metrics).toBeNull();
    expect(result.current.history).toEqual({ cpu: [], mem: [], disk: [] });
  });

  it("clears the failure once a read answers", async () => {
    let fail = true;
    installTauri({
      machine_metrics: () => {
        if (fail) throw new Error("busy");
        return { cores: 8, cpu: 12, memUsed: 8e9, memTotal: 16e9, diskUsed: 100e9, diskTotal: 500e9 };
      },
    });
    const { result, rerender } = renderHook((p: { ms: number }) => useMachineMetrics(p.ms), { initialProps: { ms: 60_000 } });
    await flush();
    expect(result.current.failed).toBe(true);
    fail = false;
    // A new interval restarts the poll with an immediate read.
    rerender({ ms: 30_000 });
    await flush();
    expect(result.current.failed).toBe(false);
    expect(result.current.metrics?.cores).toBe(8);
  });
});

describe("the stat cards when the metrics failed", () => {
  it("show a dash and say unavailable, not placeholders", () => {
    render(<UsageStats metrics={null} history={{ cpu: [], mem: [], disk: [] }} failed />);
    expect(screen.getAllByText("unavailable")).toHaveLength(3);
    expect(document.body.textContent.match(/–/g)).toHaveLength(3);
    expect(document.querySelectorAll("[data-slot=skeleton], .animate-pulse")).toHaveLength(0);
  });

  it("keep the placeholders while the first read is still on its way", () => {
    render(<UsageStats metrics={null} history={{ cpu: [], mem: [], disk: [] }} />);
    expect(screen.queryByText("unavailable")).toBeNull();
    expect(document.body.textContent).not.toContain("–");
  });

  it("a card with no value and none coming shows the dash", () => {
    render(<StatCard label="CPU" detail="unavailable" value={null} unavailable />);
    expect(document.body.textContent).toContain("–");
    expect(document.body.textContent).toContain("unavailable");
  });
});
