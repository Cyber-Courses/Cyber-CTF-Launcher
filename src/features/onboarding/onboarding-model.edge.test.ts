import { describe, expect, it } from "vitest";
import { machineSteps } from "@/features/machine/setup-steps/steps";
import { STEP_NAMES, isMachineStep, onboardingSteps } from "@/features/onboarding/onboarding-model";
import type { SystemReport } from "@/lib/tauri";

const report = (os: string) => ({ os }) as SystemReport;

describe("onboarding model: per OS", () => {
  it("lists the steps of each OS, with the virtualization step on Windows only, right after the package manager", () => {
    const common = ["docker", "docker-test", "attack", "vm", "vagrant", "vm-test"];
    expect(onboardingSteps(machineSteps(report("windows")))).toEqual(["welcome", "signin", "pkgmgr", "virtualization", ...common, "done"]);
    for (const os of ["macos", "linux", "freebsd"])
      expect(onboardingSteps(machineSteps(report(os))), os).toEqual(["welcome", "signin", "pkgmgr", ...common, "done"]);
    expect(onboardingSteps(machineSteps(null))).toEqual(["welcome", "signin", "pkgmgr", ...common, "done"]);
  });

  it("has a name for every step of every OS, and tells the machine steps from the wrapper ones", () => {
    for (const os of ["windows", "macos", "linux"]) {
      const steps = onboardingSteps(machineSteps(report(os)));
      for (const step of steps) expect(STEP_NAMES[step], `${os} ${step}`).toMatch(/^onboarding\.stepNames\./);
      expect(steps.filter(isMachineStep)).toEqual(machineSteps(report(os)));
      expect(steps.filter((s) => !isMachineStep(s))).toEqual(["welcome", "signin", "done"]);
    }
  });
});
