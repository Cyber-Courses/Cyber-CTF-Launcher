import { describe, expect, it } from "vitest";

import { rowFacts, rowPhase, runningUnlisted } from "@/features/labs/lab-row-state";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

const status = (p: Partial<LabStatus>) => ({ running: false, parked: null, ...p }) as LabStatus;
const phase = (p: Partial<Parameters<typeof rowPhase>[0]>) => rowPhase({ status: status({}), busy: false, deploying: false, solved: false, ...p });

describe("rowPhase", () => {
  it("puts an operation of this session first, even on a running lab", () => {
    expect(phase({ busy: true, status: status({ running: true }) })).toBe("busy");
  });
  it("reads a backend deploy as starting until the lab runs", () => {
    expect(phase({ deploying: true })).toBe("starting");
    expect(phase({ deploying: true, status: status({ running: true }) })).toBe("running");
  });
  it("tells a paused lab from a shut-down one", () => {
    expect(phase({ status: status({ parked: "pause" }) })).toBe("paused");
    expect(phase({ status: status({ parked: "shutdown" }) })).toBe("shutDown");
  });
  it("says a start failed, until the lab runs or something else runs on it", () => {
    expect(phase({ failed: true })).toBe("failed");
    expect(phase({ failed: true, solved: true })).toBe("failed");
    expect(phase({ failed: true, status: status({ running: true }) })).toBe("running");
    expect(phase({ failed: true, busy: true })).toBe("busy");
    expect(phase({ failed: true, deploying: true })).toBe("starting");
  });
  it("lists the labs running that the catalogue doesn't", () => {
    const lab = { id: "a" } as Lab;
    const s = { a: status({ running: true }), b: status({ running: true }), c: status({}) };
    expect(runningUnlisted([lab], s)).toEqual(["b"]);
    expect(runningUnlisted(null, s)).toEqual(["a", "b"]);
  });
  it("is solved or not started at rest", () => {
    expect(phase({ solved: true })).toBe("solved");
    expect(phase({ status: undefined })).toBe("notStarted");
  });
});

describe("rowFacts", () => {
  const x86Vm = { runtime: "VM" as const, architectures: ["x86_64"], providers: [] };
  const x86Docker = { runtime: "DOCKER" as const, architectures: ["x86_64"], providers: [] };
  const facts = (p: Partial<Parameters<typeof rowFacts>[0]>) => rowFacts({ runtime: x86Vm, status: undefined, hostArch: "arm64", emulates: false, ...p });

  it("reads a VM for another CPU as emulated only with an emulator ready", () => {
    expect(facts({}).emulated).toBe(false);
    expect(facts({ emulates: true }).emulated).toBe(true);
    expect(facts({ runtime: x86Docker }).emulated).toBe(true);
  });
  it("is never emulated on an unknown host, for any CPU or without a runtime", () => {
    expect(facts({ hostArch: "", emulates: true }).emulated).toBe(false);
    expect(facts({ runtime: { ...x86Vm, architectures: [] }, emulates: true }).emulated).toBe(false);
    expect(facts({ runtime: null }).emulated).toBe(false);
    expect(facts({ hostArch: "x86_64", runtime: x86Docker }).emulated).toBe(false);
  });
  it("reads parked only on a lab that isn't running", () => {
    expect(facts({ status: status({ parked: "pause" }) })).toMatchObject({ running: false, parked: "pause" });
    expect(facts({ status: status({ running: true, parked: "pause" }) })).toMatchObject({ running: true, parked: false });
    expect(facts({})).toMatchObject({ running: false, parked: null });
  });
});
