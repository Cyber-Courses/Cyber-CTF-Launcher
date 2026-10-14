import { describe, expect, it } from "vitest";
import { rowPhase } from "@/features/labs/lab-row-state";
import type { LabStatus } from "@/lib/tauri";

const status = (p: Partial<LabStatus>) => p as LabStatus;

// Contradictory facts about one lab: the row picks the most pressing one.
describe("rowPhase: contradictions", () => {
  it("a parked lab that is solved reads as parked, and a busy solved lab as busy", () => {
    expect(rowPhase({ status: status({ parked: "pause" }), busy: false, deploying: false, solved: true })).toBe("paused");
    expect(rowPhase({ status: status({ parked: "shutdown" }), busy: false, deploying: false, solved: true })).toBe("shutDown");
    expect(rowPhase({ status: undefined, busy: true, deploying: false, solved: true })).toBe("busy");
  });

  it("a backend deploy on a lab reported running is running; on one both parked and running too", () => {
    expect(rowPhase({ status: status({ running: true }), busy: false, deploying: true, solved: false })).toBe("running");
    expect(rowPhase({ status: status({ running: true, parked: "pause" }), busy: false, deploying: false, solved: false })).toBe("running");
    expect(rowPhase({ status: status({ parked: "pause" }), busy: false, deploying: true, solved: false })).toBe("starting");
  });
});
