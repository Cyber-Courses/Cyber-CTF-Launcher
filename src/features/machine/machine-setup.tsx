"use client";

import { useState } from "react";
import { ArrowLeft, Play, Sparkles } from "lucide-react";
import { ErrorBoundary } from "@/components/error-screen";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { CheckFailed } from "@/features/machine/check-failed";
import {
  MachineStepBody,
  SetupOutcome,
  canContinue,
  machineSteps,
  nextLabel,
  nextVariant,
  stepMeta,
  useMachineSetup,
  type MachineSetupState,
  type MachineStep,
} from "@/features/machine/setup-steps";
import type { SystemReport } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { useStepper } from "@/lib/use-stepper";

/** A step of the window: a machine setup step, then the summary. */
type WindowStep = MachineStep | "ready";

/** A request to open at a step (a "Fix" link from the Machine page); a new nonce jumps again. */
type StartAt = { step: string; nonce: number } | null;

/** The window's steps and where the player is, jumping to `startAt` once the report is in (the
 *  step list depends on it). */
function useWindowSteps(report: SystemReport | null, startAt: StartAt) {
  const steps: WindowStep[] = [...machineSteps(report), "ready"];
  const { pos, next, back, goTo } = useStepper(steps.length);
  const [jumped, setJumped] = useState<number | null>(null);
  if (startAt && report && startAt.nonce !== jumped) {
    setJumped(startAt.nonce);
    const n = (steps as string[]).indexOf(startAt.step);
    if (n >= 0) goTo(n);
  }
  return { steps, at: pos, key: steps[pos], next, back };
}

/** The guided "set up this machine" window. The steps themselves are shared with the
 *  onboarding (`setup-steps`); this file is only the window's frame. */
export function MachineSetup({
  report,
  checkError = null,
  onRetryCheck,
  onRefresh,
  onClose,
  startAt = null,
}: {
  report: SystemReport | null;
  /** The machine check failed and none answered before it: the error, with Try again. */
  checkError?: string | null;
  onRetryCheck?: () => unknown;
  onRefresh: () => void;
  onClose: () => void;
  /** Open at this step (a "Fix" link from the Machine page); changes jump again. */
  startAt?: StartAt;
}) {
  const t = useT();
  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      {/* Overlay title bar: drags the window and clears the macOS traffic lights. */}
      <div data-tauri-drag-region className="h-11 shrink-0 select-none" />

      {/* Until the machine check answers, just say so: the title, stepper and step count all
          depend on what it finds. */}
      {report === null && checkError && (
        <div className="flex flex-1 items-center justify-center px-6 pb-11">
          <CheckFailed error={checkError} onRetry={onRetryCheck ?? onRefresh} className="w-full max-w-lg" />
        </div>
      )}
      {report === null && !checkError && (
        <div className="flex flex-1 items-center justify-center gap-2 pb-11 text-[0.8125rem] text-muted-foreground">
          <Spinner className="size-4" /> {t("machine.setup.checking")}
        </div>
      )}
      {report !== null && <SetupWizard report={report} startAt={startAt} onRefresh={onRefresh} onClose={onClose} />}
    </div>
  );
}

const OS_NAMES: Partial<Record<string, string>> = { windows: "Windows", macos: "macOS" };

/** The steps once the report is in. The step's panel fills the window (up to 56rem, centred in a
 *  taller one): its body scrolls and its footer stays at the bottom, so Back / Continue are always
 *  in view and in the same place from step to step. The window scrolls only if it is shorter still. */
function SetupWizard({ report, startAt, onRefresh, onClose }: { report: SystemReport; startAt: StartAt; onRefresh: () => void; onClose: () => void }) {
  const t = useT();
  const { steps, at, key, next, back } = useWindowSteps(report, startAt);
  const setup = useMachineSetup(report, onRefresh);
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <main className="mx-auto my-auto flex max-h-[56rem] min-h-[26rem] w-full max-w-2xl flex-1 flex-col px-6 pt-2 pb-6">
        <PageHeader
          title={<>{t.rich("machine.setup.title", { em: (s) => <em>{s}</em> })}</>}
          lead={t("machine.setup.lead", { os: OS_NAMES[report.os] ?? "Linux" })}
        />
        {/* Progress: one segment per step, the jewel up to the current one. */}
        <div className="mt-5 flex gap-1" aria-hidden>
          {steps.map((s, n) => (
            <div key={s} className={cn("h-1 flex-1 rounded-full transition-colors", n <= at ? "meter-fill" : "bg-border")} />
          ))}
        </div>
        <Panel key={key} className="mt-5 flex min-h-0 flex-1 animate-rise-in flex-col">
          <StepBody step={key} report={report} setup={setup} count={{ current: at + 1, total: steps.length }} />
          <div className="flex shrink-0 items-center justify-between gap-2 border-t border-border px-4 py-3">
            <div>
              {at > 0 && (
                <Button variant="outline" size="sm" onClick={back} disabled={setup.busy}>
                  <ArrowLeft className="size-3.5" /> {t("machine.setup.back")}
                </Button>
              )}
            </div>
            {key === "ready" ? (
              <Button variant="primary" size="sm" onClick={onClose}>
                <Play className="size-3.5" /> {t("machine.setup.done")}
              </Button>
            ) : (
              <Button variant={nextVariant(key, report, setup)} size="sm" onClick={next} disabled={setup.busy || !canContinue(key, report, setup)}>
                {nextLabel(key, report, t, setup)}
              </Button>
            )}
          </div>
        </Panel>
      </main>
    </div>
  );
}

/** The step's header (icon, title, "n of N") and its scrolling body: a setup step, or the summary. */
function StepBody({
  step,
  report,
  setup,
  count,
}: {
  step: WindowStep;
  report: SystemReport;
  setup: MachineSetupState;
  count: { current: number; total: number };
}) {
  const t = useT();
  const meta =
    step === "ready" ? { icon: Sparkles, title: t("machine.setup.readyTitle"), description: t("machine.setup.readyDescription") } : stepMeta(step, report, t);
  return (
    <>
      <PanelHeader
        className="shrink-0"
        title={
          <>
            <meta.icon className="size-4 shrink-0 text-muted-foreground" />
            {meta.title}
          </>
        }
        meta={<span className="tabular-nums">{t("machine.setup.stepOf", count)}</span>}
      />
      <div data-step-body className="min-h-0 flex-1 overflow-y-auto px-4 pt-3.5 pb-4">
        <p className="text-[0.8125rem] leading-relaxed text-muted-foreground">{meta.description}</p>
        <div className="mt-4">
          <ErrorBoundary resetKey={step} title={t("machine.setup.stepFailed")}>
            {step === "ready" ? <SetupOutcome report={report} setup={setup} /> : <MachineStepBody step={step} report={report} setup={setup} />}
          </ErrorBoundary>
        </div>
      </div>
    </>
  );
}
