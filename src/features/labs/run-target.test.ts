import { describe, expect, it } from "vitest";
import { isLocalVmForm, type RunTarget } from "@/features/labs/run-target";
import { vmFormTarget } from "@/features/labs/lab-runtime";

describe("run targets", () => {
  it("tells this machine's VM forms from every other place", () => {
    const vmForms: RunTarget[] = [vmFormTarget("local-vm", "qemu"), vmFormTarget("local-vms", "utm")];
    expect(vmForms).toEqual([
      { kind: "local-vm", provider: "qemu" },
      { kind: "local-vms", provider: "utm" },
    ]);
    expect(vmForms.every(isLocalVmForm)).toBe(true);
    const others: RunTarget[] = [{ kind: "local" }, { kind: "local", provider: "qemu" }, { kind: "host", id: "aws1", perMachine: true }];
    expect(others.some(isLocalVmForm)).toBe(false);
  });
});
