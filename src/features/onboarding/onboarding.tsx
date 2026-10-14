"use client";

import { useEffect, useState } from "react";
import { ErrorBoundary } from "@/components/error-screen";
import { CtfMark } from "@/components/brand/mark";
import { Button } from "@/components/ui/button";
import { WaitingForCheck } from "@/components/ui/waiting-for-check";
import { MachineStepBody, canContinue, machineSteps, nextLabel, stepMeta, useMachineSetup } from "@/features/machine/setup-steps";
import { STEP_NAMES, isMachineStep, onboardingSteps } from "@/features/onboarding/onboarding-model";
import { StepActions, StepHeader, StepProgress } from "@/features/onboarding/onboarding-parts";
import { DoneStep, SignInStep, WelcomeStep } from "@/features/onboarding/onboarding-steps";
import { useAuthActions } from "@/features/account/account";
import { authStatus, systemCheck, type AuthStatus, type SystemReport } from "@/lib/tauri";
import { usePoll } from "@/lib/use-poll";
import { useT } from "@/lib/i18n";

/** Machine checks while onboarding: an install finished in a native installer shows up on its own. */
const CHECK_POLL_MS = 5000;

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const t = useT();
  const [i, setI] = useState(0);
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [report, setReport] = useState<SystemReport | null>(null);
  // The app's one sign-in: on failure the step stays and says why (a timeout, a missing browser
  // or an unsaved session).
  const { login, busy: loggingIn, error: loginError } = useAuthActions(setAuth);

  useEffect(() => {
    authStatus()
      .then(setAuth)
      .catch(() => setAuth(null));
  }, []);
  // Poll so an install finished in a native installer is picked up without a re-check. A failed
  // re-check keeps the last report: clearing it blanked the current step to the loader and, on
  // Windows, dropped the WSL step from the list, so the player landed on another step.
  // A check that keeps failing (a wedged daemon, a hung tool) is said, with a retry: the loader
  // alone sat there for good.
  const [checkError, setCheckError] = useState<string | null>(null);
  const refreshReport = usePoll(systemCheck, CHECK_POLL_MS, {
    serial: true,
    onValue: (r) => {
      setReport(r);
      setCheckError(null);
    },
    onError: (e) => setCheckError(String(e)),
  });

  // The machine steps are the same ones the "Set up this machine" window shows.
  const setup = useMachineSetup(report, refreshReport);
  const steps = onboardingSteps(machineSteps(report));
  const pos = Math.min(i, steps.length - 1);
  const step = steps[pos];
  const next = () => setI((n) => Math.min(n + 1, steps.length - 1));
  const back = () => setI((n) => Math.max(n - 1, 0));

  const meta = isMachineStep(step) && report !== null ? stepMeta(step, report, t) : null;

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-background">
      {/* Draggable title bar (Overlay style has no native bar); room for traffic lights. */}
      <div data-tauri-drag-region className="flex h-11 shrink-0 items-center justify-end pr-4 pl-20">
        {step !== "done" && (
          <Button variant="ghost" size="xs" onClick={onComplete}>
            {t("onboarding.skip")}
          </Button>
        )}
      </div>

      {/* One fixed-height column (centred in a tall window): the step scrolls inside it and its
          footer is pinned to the bottom, so the buttons stay in view and in place from step to
          step. The scroller spans the window, so its scrollbar sits on the window's edge. */}
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="my-auto flex max-h-[50rem] min-h-0 flex-1 flex-col overflow-y-auto px-6">
          <div className="mx-auto flex min-h-full w-full max-w-lg flex-col pt-6">
            {/* The mark, then the step position: dots and a mono "3 / 9 · Step name". */}
            <div className="flex shrink-0 flex-col items-center">
              <CtfMark className="size-12" />
              <StepProgress count={steps.length} pos={pos} icon={meta?.icon} name={t(STEP_NAMES[step])} />
            </div>

            <div key={step} className="mt-8 flex flex-1 animate-rise-in flex-col">
              {step === "welcome" && <WelcomeStep onNext={next} />}

              {step === "signin" && <SignInStep auth={auth} loggingIn={loggingIn} loginError={loginError} onLogin={login} onBack={back} onNext={next} />}

              {/* A machine step before the machine check answers: the loader, or why it failed. */}
              {isMachineStep(step) && report === null && (
                <div className="flex flex-1 flex-col">
                  <div className="flex items-center justify-center gap-2 text-[0.8125rem] text-muted-foreground">
                    <WaitingForCheck what={t("onboarding.checking")} checkError={checkError} onRetry={refreshReport} />
                  </div>
                  {/* Back still works (to sign in, say): without it "Skip setup" was the only way out. */}
                  <StepActions onBack={back} onNext={next} disabled nextLabel={t("machine.steps.next.continue")} />
                </div>
              )}

              {isMachineStep(step) && report !== null && meta && (
                <div className="flex flex-1 flex-col">
                  <StepHeader title={meta.title} description={meta.description} />
                  <div className="mt-8">
                    <ErrorBoundary resetKey={step} title={t("onboarding.stepFailed")}>
                      <MachineStepBody step={step} report={report} setup={setup} />
                    </ErrorBoundary>
                  </div>
                  <StepActions
                    onBack={back}
                    backDisabled={setup.busy}
                    onNext={next}
                    disabled={setup.busy || !canContinue(step, report, setup)}
                    nextLabel={nextLabel(step, report, t, setup)}
                  />
                </div>
              )}

              {step === "done" && <DoneStep auth={auth} report={report} setup={setup} onComplete={onComplete} />}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
