"use client";

import { useEffect, useState } from "react";
import { ErrorBoundary } from "@/components/error-screen";
import { CtfMark } from "@/components/brand/mark";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { MachineStepBody, canContinue, machineSteps, nextLabel, stepMeta, useMachineSetup } from "@/features/machine/setup-steps";
import { STEP_NAMES, isMachineStep, onboardingSteps } from "@/features/onboarding/onboarding-model";
import { StepActions, StepHeader, StepProgress } from "@/features/onboarding/onboarding-parts";
import { DoneStep, SignInStep, WelcomeStep } from "@/features/onboarding/onboarding-steps";
import { authLogin, authStatus, systemCheck, type AuthStatus, type SystemReport } from "@/lib/tauri";
import { usePoll } from "@/lib/use-poll";
import { useT } from "@/lib/i18n";

/** Machine checks while onboarding: an install finished in a native installer shows up on its own. */
const CHECK_POLL_MS = 5000;

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const t = useT();
  const [i, setI] = useState(0);
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [report, setReport] = useState<SystemReport | null>(null);
  const [loggingIn, setLoggingIn] = useState(false);
  const [loginError, setLoginError] = useState<string | null>(null);

  useEffect(() => {
    authStatus()
      .then(setAuth)
      .catch(() => setAuth(null));
  }, []);
  // Poll so an install finished in a native installer is picked up without a re-check.
  const refreshReport = usePoll(systemCheck, CHECK_POLL_MS, { serial: true, onValue: setReport, onError: () => setReport(null) });

  // The machine steps are the same ones the "Set up this machine" window shows.
  const setup = useMachineSetup(report, refreshReport);
  const steps = onboardingSteps(machineSteps(report));
  const pos = Math.min(i, steps.length - 1);
  const step = steps[pos];
  const next = () => setI((n) => Math.min(n + 1, steps.length - 1));
  const back = () => setI((n) => Math.max(n - 1, 0));

  async function login() {
    setLoggingIn(true);
    setLoginError(null);
    try {
      setAuth(await authLogin());
    } catch (e) {
      // Stay on this step, and say why: a timeout, a missing browser or an unsaved session.
      setLoginError(String(e));
    } finally {
      setLoggingIn(false);
    }
  }

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

      <div className="flex flex-1 justify-center overflow-y-auto px-6 pt-6 pb-12">
        <div className="my-auto w-full max-w-lg">
          {/* The mark, then the step position: dots and a mono "3 / 9 · Step name". */}
          <div className="flex flex-col items-center">
            <CtfMark className="size-12" />
            <StepProgress count={steps.length} pos={pos} icon={meta?.icon} name={t(STEP_NAMES[step])} />
          </div>

          <div key={step} className="mt-8 animate-rise-in">
            {step === "welcome" && <WelcomeStep onNext={next} />}

            {step === "signin" && <SignInStep auth={auth} loggingIn={loggingIn} loginError={loginError} onLogin={login} onBack={back} onNext={next} />}

            {/* A machine step before the machine check answers: just the loader, no step header. */}
            {isMachineStep(step) && report === null && (
              <div className="flex items-center justify-center gap-2 text-[0.8125rem] text-muted-foreground">
                <Spinner className="size-4" /> {t("onboarding.checking")}
              </div>
            )}

            {isMachineStep(step) && report !== null && meta && (
              <div>
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
                  nextLabel={nextLabel(step, report, t)}
                />
              </div>
            )}

            {step === "done" && <DoneStep auth={auth} report={report} setup={setup} onComplete={onComplete} />}
          </div>
        </div>
      </div>
    </div>
  );
}
