import { describe, expect, it } from "vitest";
import { isLocalVmForm, type RunTarget } from "@/features/labs/run-target";
import { vmFormTarget } from "@/features/labs/lab-runtime";

describe("run targets", () => {
  it("tells this machine's VM form of a container lab from every other place", () => {
    const vmForm: RunTarget = vmFormTarget("local-vms", "qemu");
    expect(vmForm).toEqual({ kind: "local-vms", provider: "qemu" });
    expect(isLocalVmForm(vmForm)).toBe(true);
    const others: RunTarget[] = [{ kind: "local" }, { kind: "local", provider: "qemu" }, { kind: "host", id: "aws1", perMachine: true }];
    expect(others.some(isLocalVmForm)).toBe(false);
  });
});
