"use client";

import { ErrorBoundary } from "@/components/error-screen";

import { useState } from "react";
import { ArrowLeft, Play, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { CheckFailed } from "@/features/machine/check-failed";
import { MachineStepBody, SetupOutcome, canContinue, machineSteps, nextLabel, stepMeta, useMachineSetup } from "@/features/machine/setup-steps";
import type { SystemReport } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** The guided "set up this machine" window. The steps themselves are shared with the
 *  onboarding (`setup-steps`); this file is only the window's frame. */
export function MachineSetup({
  report,
  checkError = null,
  onRetryCheck,
  onRefresh,
  onClose,
  startAt,
}: {
  report: SystemReport | null;
  /** The machine check failed and none answered before it: the error, with Try again. */
  checkError?: string | null;
  onRetryCheck?: () => unknown;
  onRefresh: () => void;
  onClose: () => void;
  /** Open at this step (a "Fix" link from the Machine page); changes jump again. */
  startAt?: { step: string; nonce: number } | null;
}) {
  const t = useT();
  const steps = [...machineSteps(report), "ready" as const];
  const [i, setI] = useState(0);
  // Jump to the requested step once the report is in (the step list depends on it).
  const [jumped, setJumped] = useState<number | null>(null);
  if (startAt && report && startAt.nonce !== jumped) {
    setJumped(startAt.nonce);
    const n = (steps as string[]).indexOf(startAt.step);
    if (n >= 0) setI(n);
  }
  const at = Math.min(i, steps.length - 1);
  const key = steps[at];
  const setup = useMachineSetup(report, onRefresh);
  const next = () => setI(Math.min(at + 1, steps.length - 1));
  const back = () => setI(Math.max(at - 1, 0));

  const osName = report?.os === "windows" ? "Windows" : report?.os === "macos" ? "macOS" : "Linux";
  const meta =
    key === "ready" ? { icon: Sparkles, title: t("machine.setup.readyTitle"), description: t("machine.setup.readyDescription") } : stepMeta(key, report, t);

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      {/* Overlay title bar: drags the window and clears the macOS traffic lights. */}
      <div data-tauri-drag-region className="h-11 shrink-0 select-none" />

      {/* Until the machine check answers, just say so: the title, stepper and step count all
          depend on what it finds. */}
      {!report && checkError ? (
        <div className="flex flex-1 items-center justify-center px-6 pb-11">
          <CheckFailed error={checkError} onRetry={onRetryCheck ?? onRefresh} className="w-full max-w-lg" />
        </div>
      ) : !report ? (
        <div className="flex flex-1 items-center justify-center gap-2 pb-11 text-[0.8125rem] text-muted-foreground">
          <Spinner className="size-4" /> {t("machine.setup.checking")}
        </div>
      ) : (
        /* The step's panel fills the window (up to 56rem, centred in a taller one): its body
           scrolls and its footer stays at the bottom, so Back / Continue are always in view and
           in the same place from step to step. The window scrolls only if it is shorter still. */
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <main className="mx-auto my-auto flex max-h-[56rem] min-h-[26rem] w-full max-w-2xl flex-1 flex-col px-6 pt-2 pb-6">
            <PageHeader title={<>{t.rich("machine.setup.title", { em: (s) => <em>{s}</em> })}</>} lead={t("machine.setup.lead", { os: osName })} />

            {/* Progress: one segment per step, the jewel up to the current one. */}
            <div className="mt-5 flex gap-1" aria-hidden>
              {steps.map((_, n) => (
                <div key={n} className={cn("h-1 flex-1 rounded-full transition-colors", n <= at ? "meter-fill" : "bg-border")} />
              ))}
            </div>

            <Panel key={key} className="mt-5 flex min-h-0 flex-1 animate-rise-in flex-col">
              <PanelHeader
                className="shrink-0"
                title={
                  <>
                    <meta.icon className="size-4 shrink-0 text-muted-foreground" />
                    {meta.title}
                  </>
                }
                meta={<span className="tabular-nums">{t("machine.setup.stepOf", { current: at + 1, total: steps.length })}</span>}
              />
              <div data-step-body className="min-h-0 flex-1 overflow-y-auto px-4 pt-3.5 pb-4">
                <p className="text-[0.8125rem] leading-relaxed text-muted-foreground">{meta.description}</p>
                <div className="mt-4">
                  <ErrorBoundary resetKey={key} title={t("machine.setup.stepFailed")}>
                    {key === "ready" ? <SetupOutcome report={report} setup={setup} /> : <MachineStepBody step={key} report={report} setup={setup} />}
                  </ErrorBoundary>
                </div>
              </div>
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
                  <Button variant="primary" size="sm" onClick={next} disabled={setup.busy || !canContinue(key, report, setup)}>
                    {nextLabel(key, report, t)}
                  </Button>
                )}
              </div>
            </Panel>
          </main>
        </div>
      )}
    </div>
  );
}
