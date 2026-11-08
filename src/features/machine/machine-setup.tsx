"use client";

import type { ReactNode } from "react";
import { ArrowLeft, Compass, Play, Sparkles } from "lucide-react";
import { ErrorBoundary } from "@/components/error-screen";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { CheckFailed } from "@/features/machine/check-failed";
import {
  MachineStepBody,
  PathChooser,
  SetupOutcome,
  canContinue,
  nextLabel,
  nextVariant,
  stepMeta,
  useMachineSetup,
  type MachineSetupState,
  type SetupPath,
} from "@/features/machine/setup-steps";
import { useWindowSteps, type StartAt, type WindowStep } from "@/features/machine/use-window-steps";
import { showInMainWindow } from "@/lib/app-events";
import { getLastTest } from "@/lib/settings";
import type { SystemReport } from "@/lib/tauri";
import { useVmProvider } from "@/lib/use-vm-provider";
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

/** The steps once the report is in: the path chooser, then one path's steps and its summary.
 *  The step's panel fills the window (up to 56rem, centred in a taller one): its body scrolls and
 *  its footer stays at the bottom, so Back / Continue are always in view and in the same place
 *  from step to step. The window scrolls only if it is shorter still. */
function SetupWizard({ report, startAt, onRefresh, onClose }: { report: SystemReport; startAt: StartAt; onRefresh: () => void; onClose: () => void }) {
  const t = useT();
  const last = { docker: getLastTest("docker"), vm: getLastTest("vm") };
  const preferred = useVmProvider();
  const { steps, path, at, key, next, back, start } = useWindowSteps(report, startAt, last, preferred);
  const setup = useMachineSetup(report, onRefresh);
  // The path's steps and the summary (the chooser, first in the list, counts for neither).
  const walk = steps.slice(1);
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <main className="mx-auto my-auto flex max-h-[56rem] min-h-[26rem] w-full max-w-2xl flex-1 flex-col px-6 pt-2 pb-6">
        <PageHeader
          title={<>{t.rich("machine.setup.title", { em: (s) => <em>{s}</em> })}</>}
          lead={t("machine.setup.lead", { os: OS_NAMES[report.os] ?? "Linux" })}
        />
        {/* Progress along the path: one segment per step, the jewel up to the current one. */}
        {key !== "path" && (
          <div className="mt-5 flex gap-1" aria-hidden>
            {walk.map((s, n) => (
              <div key={s} className={cn("h-1 flex-1 rounded-full transition-colors", n < at ? "meter-fill" : "bg-border")} />
            ))}
          </div>
        )}
        <Panel key={key} className="mt-5 flex min-h-0 flex-1 animate-rise-in flex-col">
          <StepBody step={key} report={report} setup={setup} path={path} count={{ current: at, total: walk.length }}>
            {key === "path" && (
              <PathChooser
                report={report}
                last={last}
                preferred={preferred}
                onStart={start}
                // Cloud accounts are connected on the Cloud page: show it and leave this window.
                onCloud={() => {
                  showInMainWindow("cloud");
                  onClose();
                }}
              />
            )}
          </StepBody>
          {key !== "path" && (
            <div className="flex shrink-0 items-center justify-between gap-2 border-t border-border px-4 py-3">
              <Button variant="outline" size="sm" onClick={back} disabled={setup.busy}>
                <ArrowLeft className="size-3.5" /> {t("machine.setup.back")}
              </Button>
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
          )}
        </Panel>
      </main>
    </div>
  );
}

/** The step's header (icon, title, "n of N" along the path) and its scrolling body: the path
 *  chooser (`children`), a setup step, or the summary of the path walked. */
function StepBody({
  step,
  report,
  setup,
  path,
  count,
  children,
}: {
  step: WindowStep;
  report: SystemReport;
  setup: MachineSetupState;
  path: SetupPath | null;
  count: { current: number; total: number };
  children?: ReactNode;
}) {
  const t = useT();
  const meta =
    step === "path"
      ? { icon: Compass, title: t("machine.setup.pathTitle"), description: t("machine.setup.pathDescription") }
      : step === "ready"
        ? { icon: Sparkles, title: t("machine.setup.readyTitle"), description: t("machine.setup.readyDescription") }
        : stepMeta(step, report, t);
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
        meta={step !== "path" && <span className="tabular-nums">{t("machine.setup.stepOf", count)}</span>}
      />
      <div data-step-body className="min-h-0 flex-1 overflow-y-auto px-4 pt-3.5 pb-4">
        <p className="text-[0.8125rem] leading-relaxed text-muted-foreground">{meta.description}</p>
        <div className="mt-4">
          <ErrorBoundary resetKey={step} title={t("machine.setup.stepFailed")}>
            {step === "path" ? (
              children
            ) : step === "ready" ? (
              <SetupOutcome report={report} setup={setup} paths={path ? [path] : undefined} />
            ) : (
              <MachineStepBody step={step} report={report} setup={setup} />
            )}
          </ErrorBoundary>
        </div>
      </div>
    </>
  );
}
