import { describe, expect, it } from "vitest";
import { mergeScan } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

const probed = { running: false, parked: null, url: "http://127.0.0.1:8080", machines: [], networks: [] } as unknown as LabStatus;

describe("mergeScan", () => {
  it("keeps the probed statuses as they are when the scan sees nothing", () => {
    const statuses = { a: probed };
    expect(mergeScan(statuses, new Set())).toBe(statuses);
  });
  it("marks a lab the scan sees running, keeping what its probe read", () => {
    const merged = mergeScan({ a: probed, b: probed }, new Set(["a"]));
    expect(merged.a).toEqual({ ...probed, running: true });
    expect(merged.b).toBe(probed);
  });
  it("gives a lab the scan sees but no probe answered yet a bare running status", () => {
    expect(mergeScan({}, new Set(["c"])).c).toMatchObject({ running: true, parked: null, machines: [], url: null, host: null, outputs: [] });
  });
});
