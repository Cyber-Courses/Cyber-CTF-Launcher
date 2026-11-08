import type { MessageKey } from "@/lib/i18n";
import type { MachineStep } from "@/features/machine/setup-steps/steps";

export type OnboardingStep = "welcome" | "signin" | MachineStep | "done";

/** Names for the progress line. VM steps are optional: Docker labs run without them. */
export const STEP_NAMES: Record<OnboardingStep, MessageKey> = {
  welcome: "onboarding.stepNames.welcome",
  signin: "onboarding.stepNames.signin",
  pkgmgr: "onboarding.stepNames.pkgmgr",
  virtualization: "onboarding.stepNames.virtualization",
  docker: "onboarding.stepNames.docker",
  "docker-test": "onboarding.stepNames.dockerTest",
  attack: "onboarding.stepNames.attack",
  vm: "onboarding.stepNames.vm",
  vagrant: "onboarding.stepNames.vagrant",
  "vm-test": "onboarding.stepNames.vmTest",
  done: "onboarding.stepNames.done",
};

/** The onboarding: a welcome and the sign-in, the machine's own setup steps, then the summary. */
export const onboardingSteps = (machine: MachineStep[]): OnboardingStep[] => ["welcome", "signin", ...machine, "done"];

/** Whether a step is one of the machine setup steps (shared with the "Set up this machine" window). */
export const isMachineStep = (step: OnboardingStep): step is MachineStep => step !== "welcome" && step !== "signin" && step !== "done";
