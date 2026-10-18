import { describe, expect, it } from "vitest";
import { machineSteps } from "@/features/machine/setup-steps/steps";
import { STEP_NAMES, isMachineStep, onboardingSteps } from "@/features/onboarding/onboarding-model";
import type { SystemReport } from "@/lib/tauri";

describe("onboarding model", () => {
  it("wraps the machine's own setup steps in a welcome, the sign-in and a summary", () => {
    const steps = onboardingSteps(machineSteps({ os: "windows" } as SystemReport));
    expect(steps[0]).toBe("welcome");
    expect(steps[1]).toBe("signin");
    expect(steps).toContain("virtualization");
    expect(steps.at(-1)).toBe("done");
    expect(onboardingSteps(machineSteps(null))).not.toContain("virtualization");
  });

  it("names every step, and tells machine steps apart", () => {
    for (const step of onboardingSteps(machineSteps({ os: "windows" } as SystemReport))) expect(STEP_NAMES[step]).toMatch(/^onboarding\.stepNames\./);
    expect(isMachineStep("docker-test")).toBe(true);
    expect(isMachineStep("welcome")).toBe(false);
    expect(isMachineStep("signin")).toBe(false);
    expect(isMachineStep("done")).toBe(false);
  });
});
