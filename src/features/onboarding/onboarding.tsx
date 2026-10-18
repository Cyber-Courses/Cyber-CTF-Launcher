"use client";

import type { ReactNode } from "react";
import { ErrorBoundary } from "@/components/error-screen";
import { CtfMark } from "@/components/brand/mark";
import { Button } from "@/components/ui/button";
import { WaitingForCheck } from "@/features/machine/waiting-for-check";
import {
  MachineStepBody,
  canContinue,
  machineSteps,
  nextLabel,
  nextVariant,
  stepMeta,
  useMachineSetup,
  type MachineSetupState,
  type MachineStep,
} from "@/features/machine/setup-steps";
import { STEP_NAMES, isMachineStep, onboardingSteps } from "@/features/onboarding/onboarding-model";
import { StepActions, StepHeader, StepProgress } from "@/features/onboarding/onboarding-parts";
import { DoneStep, SignInStep, WelcomeStep } from "@/features/onboarding/onboarding-steps";
import { useMachineReport, useOnboardingAuth, type MachineReport } from "@/features/onboarding/use-onboarding";
import { useStepper } from "@/lib/use-stepper";
import { useT } from "@/lib/i18n";

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const t = useT();
  const signIn = useOnboardingAuth();
  const machine = useMachineReport();
  // The machine steps are the same ones the "Set up this machine" window shows.
  const setup = useMachineSetup(machine.report, machine.refreshReport);
  const steps = onboardingSteps(machineSteps(machine.report));
  const { pos, next, back } = useStepper(steps.length);
  const step = steps[pos];
  const icon = isMachineStep(step) && machine.report !== null ? stepMeta(step, machine.report, t).icon : undefined;

  return (
    <OnboardingFrame onSkip={step === "done" ? null : onComplete}>
      {/* The mark, then the step position: dots and a mono "3 / 9 · Step name". */}
      <div className="flex shrink-0 flex-col items-center">
        <CtfMark className="size-12" />
        <StepProgress count={steps.length} pos={pos} icon={icon} name={t(STEP_NAMES[step])} />
      </div>

      <div key={step} className="mt-8 flex flex-1 animate-rise-in flex-col">
        {step === "welcome" && <WelcomeStep onNext={next} />}
        {step === "signin" && (
          <SignInStep auth={signIn.auth} loggingIn={signIn.loggingIn} loginError={signIn.loginError} onLogin={signIn.login} onBack={back} onNext={next} />
        )}
        {isMachineStep(step) && <MachineStepView step={step} machine={machine} setup={setup} onBack={back} onNext={next} />}
        {step === "done" && <DoneStep auth={signIn.auth} report={machine.report} setup={setup} onComplete={onComplete} />}
      </div>
    </OnboardingFrame>
  );
}

/** The window: a draggable title bar with Skip (`onSkip`, none on the last step), and the step column. */
function OnboardingFrame({ onSkip, children }: { onSkip: (() => void) | null; children: ReactNode }) {
  const t = useT();
  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-background">
      {/* Draggable title bar (Overlay style has no native bar); room for traffic lights. */}
      <div data-tauri-drag-region className="flex h-11 shrink-0 items-center justify-end pr-4 pl-20">
        {onSkip && (
          <Button variant="ghost" size="xs" onClick={onSkip}>
            {t("onboarding.skip")}
          </Button>
        )}
      </div>

      {/* One fixed-height column (centred in a tall window): the step scrolls inside it and its
          footer is pinned to the bottom, so the buttons stay in view and in place from step to
          step. The scroller spans the window, so its scrollbar sits on the window's edge. */}
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="my-auto flex max-h-[50rem] min-h-0 flex-1 flex-col overflow-y-auto px-6">
          <div className="mx-auto flex min-h-full w-full max-w-lg flex-col pt-6">{children}</div>
        </div>
      </div>
    </div>
  );
}

/** A machine step: its header, body and buttons, or before the machine check answers, the
 *  loader (or why it failed). */
function MachineStepView({
  step,
  machine,
  setup,
  onBack,
  onNext,
}: {
  step: MachineStep;
  machine: MachineReport;
  setup: MachineSetupState;
  onBack: () => void;
  onNext: () => void;
}) {
  const t = useT();
  const { report } = machine;
  if (report === null)
    return (
      <div className="flex flex-1 flex-col">
        <div className="flex items-center justify-center gap-2 text-[0.8125rem] text-muted-foreground">
          <WaitingForCheck what={t("onboarding.checking")} checkError={machine.checkError} onRetry={machine.refreshReport} />
        </div>
        {/* Back still works (to sign in, say): without it "Skip setup" was the only way out. */}
        <StepActions onBack={onBack} onNext={onNext} disabled nextLabel={t("machine.steps.next.continue")} />
      </div>
    );
  const meta = stepMeta(step, report, t);
  return (
    <div className="flex flex-1 flex-col">
      <StepHeader title={meta.title} description={meta.description} />
      <div className="mt-8">
        <ErrorBoundary resetKey={step} title={t("onboarding.stepFailed")}>
          <MachineStepBody step={step} report={report} setup={setup} />
        </ErrorBoundary>
      </div>
      <StepActions
        onBack={onBack}
        backDisabled={setup.busy}
        onNext={onNext}
        disabled={setup.busy || !canContinue(step, report, setup)}
        nextLabel={nextLabel(step, report, t, setup)}
        nextVariant={nextVariant(step, report, setup)}
      />
    </div>
  );
}
