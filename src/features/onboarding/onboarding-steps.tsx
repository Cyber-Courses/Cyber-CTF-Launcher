"use client";

import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { SetupOutcome, type MachineSetupState } from "@/features/machine/setup-steps";
import { Feature, StepActions, StepFooter, StepHeader, SummaryRow } from "@/features/onboarding/onboarding-parts";
import type { AuthStatus, SystemReport } from "@/lib/tauri";
import { useT, type T } from "@/lib/i18n";

const em = (s: string) => <em>{s}</em>;

/** "Signed in as Ada" (or just "Signed in"). */
const signedInLabel = (t: T, auth: AuthStatus) => (auth.name ? t("onboarding.signin.signedInAs", { name: auth.name }) : t("onboarding.signin.signedIn"));

/** What Cyber CTF does, and the way in. */
export function WelcomeStep({ onNext }: { onNext: () => void }) {
  const t = useT();
  return (
    <div className="flex flex-1 flex-col">
      <StepHeader title={<>{t.rich("onboarding.welcome.title", { em })}</>} description={t("onboarding.welcome.description")} />
      <Panel className="mt-8">
        <Feature icon="container" title={t("onboarding.welcome.labsTitle")} description={t("onboarding.welcome.labsDescription")} />
        <Feature icon="cloud" title={t("onboarding.welcome.whereTitle")} description={t("onboarding.welcome.whereDescription")} />
        <Feature icon="shield" title={t("onboarding.welcome.targetsTitle")} description={t("onboarding.welcome.targetsDescription")} />
        <Feature icon="plug" title={t("onboarding.welcome.launchTitle")} description={t("onboarding.welcome.launchDescription")} />
      </Panel>
      <StepFooter className="flex justify-center">
        <Button size="lg" className="min-w-48" onClick={onNext}>
          {t("onboarding.welcome.start")} <ArrowRight className="size-4" />
        </Button>
      </StepFooter>
    </div>
  );
}

/** The browser sign-in, or who is signed in already. */
export function SignInStep({
  auth,
  loggingIn,
  loginError,
  onLogin,
  onBack,
  onNext,
}: {
  auth: AuthStatus | null;
  loggingIn: boolean;
  loginError: string | null;
  onLogin: () => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const t = useT();
  return (
    <div className="flex flex-1 flex-col">
      <StepHeader title={t("onboarding.signin.title")} description={t("onboarding.signin.description")} />
      <div className="mt-8">
        {auth?.loggedIn ? (
          <Panel className="flex items-center gap-3 px-4 py-3.5">
            <StatusDot tone="ok" />
            <div className="min-w-0">
              <p className="text-[0.8125rem] font-medium text-foreground">{signedInLabel(t, auth)}</p>
              {auth.email && <p className="truncate font-mono text-[0.6875rem] text-faint">{auth.email}</p>}
            </div>
          </Panel>
        ) : (
          <Panel className="px-5 py-5 text-center">
            <Button variant="outline" size="lg" className="w-full" onClick={onLogin} disabled={loggingIn}>
              {loggingIn ? (
                <>
                  <Spinner className="size-4" /> {t("onboarding.signin.waiting")}
                </>
              ) : (
                <>
                  <Icon name="user" className="size-4" /> {t("onboarding.signin.button")}
                </>
              )}
            </Button>
            <p className="mt-2.5 text-[0.75rem] text-muted-foreground">
              {t.rich("onboarding.signin.hint", { mono: (s) => <span className="font-mono text-[0.6875rem]">{s}</span> })}
            </p>
            {loginError && (
              <p role="alert" className="surface-log mt-4 flex items-start gap-2 rounded-control p-3 text-left text-[0.75rem] text-foreground">
                <StatusDot tone="fail" className="mt-[0.3rem]" />
                <span className="min-w-0 break-words">{loginError}</span>
              </p>
            )}
          </Panel>
        )}
      </div>
      {/* Signing in is optional (labs run without an account): said on the button, so going on
          without it reads as a choice, not a mistake. */}
      <StepActions onBack={onBack} onNext={onNext} nextLabel={auth?.loggedIn ? t("onboarding.continue") : t("onboarding.signin.skip")} />
    </div>
  );
}

/** The summary: the account and what this machine can run now. */
export function DoneStep({
  auth,
  report,
  setup,
  onComplete,
}: {
  auth: AuthStatus | null;
  report: SystemReport | null;
  setup: MachineSetupState;
  onComplete: () => void;
}) {
  const t = useT();
  return (
    <div className="flex flex-1 flex-col">
      <StepHeader title={<>{t.rich("onboarding.done.title", { em })}</>} description={t("onboarding.done.description")} />
      <div className="mt-8 space-y-2.5">
        <SummaryRow
          ok={!!auth?.loggedIn}
          label={t("onboarding.done.account")}
          value={auth?.loggedIn ? signedInLabel(t, auth) : t("onboarding.done.notSignedIn")}
        />
        <SetupOutcome report={report} setup={setup} />
      </div>
      <StepFooter className="flex justify-center">
        <Button size="lg" className="min-w-48" onClick={onComplete}>
          {t("onboarding.done.browse")} <ArrowRight className="size-4" />
        </Button>
      </StepFooter>
    </div>
  );
}
