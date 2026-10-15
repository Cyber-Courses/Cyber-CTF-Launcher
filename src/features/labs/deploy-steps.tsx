"use client";

import { useCallback, useMemo, useState } from "react";
import { PanelHeader } from "@/components/ui/panel";
import { StatusDot, StatusPill } from "@/components/ui/status-pill";
import { formatDuration } from "@/lib/format";
import type { ActiveOperation } from "@/lib/tauri";
import { deriveSteps, failedMachineStep, type Step, type Timed } from "@/features/labs/deploy-log";
import { LiveDuration, RawLog, StepItem, StepMark } from "@/features/labs/deploy-step-parts";
import { OPERATION_STATUS } from "@/features/labs/runs/operations";
import { useT, type T } from "@/lib/i18n";

type Operation = ActiveOperation["op"];

// Operations on a lab that is already built: one step named after them, not the launch's
// "Building" / "Preparing", which read as a new deploy while the lab was shutting down.
const OPERATIONS = ["resume", "pause", "shutdown", "stop", "attack_vm"] as const;

type OperationText = { busy: string; step: string; done: string };

/** An operation's words (busy, its one step, done), when it is one of OPERATIONS. */
function operationText(t: T, operation: Operation): OperationText | undefined {
  const op = OPERATIONS.find((o) => o === operation);
  if (!op) return undefined;
  return { busy: t(`labs.deploy.ops.${op}.busy`), step: t(`labs.deploy.ops.${op}.step`), done: t(`labs.deploy.ops.${op}.done`) };
}

/**
 * The run read from its log: its timed lines (each line's arrival time comes with it: the deploy
 * store stamps its own lines, `useWorkerLog` a worker's), the ✓ or ✗ line that ended it, its
 * span (`end` null while it still runs: the clock ticks in `LiveDuration` alone), and its steps
 * (built from the log per the detected target, Vagrant / Terraform / Docker, or one step named
 * after an operation on the built lab), with the one that failed. Derived again only when the
 * log changes, not on every tick of the clock.
 */
function useDeployTimeline(lines: string[], times: number[], busy: boolean, operation: Operation, startedAt: number | undefined) {
  const t = useT();
  const timed = useMemo<Timed[]>(() => lines.map((line, i) => ({ line, at: times.at(i) ?? times.at(-1) ?? 0 })), [lines, times]);
  const derived = useMemo(() => {
    const failed = timed.find((row) => row.line.startsWith("✗"));
    const done = timed.find((row) => row.line.startsWith("✓"));
    const op = operationText(t, operation);
    const steps = op ? (timed.length > 0 ? [{ id: operation, label: op.step, rows: timed }] : []) : deriveSteps(t, timed);
    const lastSeen = steps.at(-1)?.id;
    const named = failed ? failedMachineStep(failed.line) : null;
    const failedStep = failed ? (named && steps.some((s) => s.id === named) ? named : lastSeen) : null;
    // From when the run began, when known: a run that failed on its first line otherwise read 0.0s.
    const first = timed.at(0)?.at;
    const start = startedAt !== undefined ? Math.min(startedAt, first ?? startedAt) : first;
    return { timed, failed, done, start, op, steps, lastSeen, failedStep };
  }, [t, timed, operation, startedAt]);
  const { done, failed } = derived;
  const end: number | null | undefined = done?.at ?? failed?.at ?? (busy ? null : timed.at(-1)?.at);
  return { ...derived, end };
}

/** The panel's header: where it deploys, its state (failed, starting, ready, done) and how long. */
function DeployHeader({
  where,
  timeline: { failed, done, start, end, op, timed },
  busy,
  ready,
  operation,
}: {
  where?: string;
  timeline: ReturnType<typeof useDeployTimeline>;
  busy: boolean;
  ready: boolean;
  operation: Operation;
}) {
  const t = useT();
  return (
    <PanelHeader
      title={
        <>
          {t("labs.deploy.title")}
          {where && (
            <span className="truncate font-normal text-muted-foreground">
              {t.rich("labs.deploy.on", { where: (s) => <span className="text-foreground">{s}</span> }, { where })}
            </span>
          )}
        </>
      }
      action={
        <span className="flex items-center gap-2.5">
          {failed ? (
            <StatusPill tone="fail">{t("labs.deploy.failed")}</StatusPill>
          ) : busy ? (
            <StatusPill tone="warn" pulse>
              {op?.busy ?? busyWord(t, operation)}
            </StatusPill>
          ) : ready || (done && !op) ? (
            <StatusPill tone="ok">{t("labs.deploy.ready")}</StatusPill>
          ) : op && timed.length > 0 ? (
            <StatusPill tone="muted">{op.done}</StatusPill>
          ) : null}
          {start && end !== undefined && (
            <span className="font-mono text-[0.6875rem] tabular-nums text-faint">
              {end === null ? <LiveDuration from={start} /> : formatDuration(end - start)}
            </span>
          )}
        </span>
      }
    />
  );
}

/** A launch's or a provisioning run's word while it runs: the one word a start has everywhere. */
const busyWord = (t: T, operation: Operation) =>
  operation === "provision" ? t("common.operation.label.provision").replace(/…$/, "") : (OPERATION_STATUS.launch ?? t("common.operation.status.launch"));

/** When a step's last line came: a step holds at least the line that made it. */
const lastAt = (step: Step) => step.rows[step.rows.length - 1].at;

/** The steps, then Ready once done; before the first step shows, the step the backend reports
 *  (a run with no log here) or a pending Preparing row. */
function StepList({ timeline: tl, busy, liveStep }: { timeline: ReturnType<typeof useDeployTimeline>; busy: boolean; liveStep?: string | null }) {
  const t = useT();
  const [open, setOpen] = useState<string | null>(null);
  const { steps, done, failed, op } = tl;
  return (
    <ul className="py-1 empty:hidden">
      {steps.map((p, i) => {
        const next = steps.at(i + 1)?.rows[0].at;
        const state = p.id === tl.failedStep ? "fail" : p.id === tl.lastSeen && busy ? "running" : "ok";
        const to = next ?? (state === "running" ? null : (done?.at ?? failed?.at ?? lastAt(p)));
        const expanded = open === p.id;
        return (
          <StepItem
            key={p.id}
            step={p}
            state={state}
            // The first step from the run's start: what happened before its first line is its own.
            from={i === 0 && tl.start !== undefined ? Math.min(tl.start, p.rows[0].at) : p.rows[0].at}
            to={to}
            origin={tl.start ?? p.rows[0].at}
            expanded={expanded}
            onToggle={() => {
              setOpen(expanded ? null : p.id);
            }}
          />
        );
      })}
      {done && !op && (
        <li className="grid grid-cols-[1.25rem_minmax(0,1fr)] items-center gap-3 px-4 py-2 text-[0.8125rem]">
          <StepMark state="ok" />
          <span className="text-foreground">{t("labs.deploy.ready")}</span>
        </li>
      )}
      {busy && steps.length === 0 && liveStep && (
        <li className="grid grid-cols-[1.25rem_minmax(0,1fr)_auto] items-center gap-3 px-4 py-2 text-[0.8125rem]">
          <StepMark state="running" />
          <span key={liveStep} className="animate-fade-in truncate text-foreground" title={liveStep}>
            {liveStep}
          </span>
        </li>
      )}
      {busy && steps.length === 0 && !liveStep && (
        <li className="grid grid-cols-[1.25rem_minmax(0,1fr)] items-center gap-3 px-4 py-2 text-[0.8125rem] text-muted-foreground">
          <StepMark state="pending" />
          {op ? `${op.busy}…` : t("labs.deploy.preparingEllipsis")}
        </li>
      )}
    </ul>
  );
}

/** A lab's start-up as named steps with their durations, Vercel-build style (steps read from the
 *  log in deploy-log.ts, timed when each line arrives). The raw log stays one click away. */
export function DeploySteps({
  lines,
  times,
  busy,
  ready,
  where,
  operation = "launch",
  startedAt,
  liveStep,
}: {
  lines: string[];
  /** When each line arrived, parallel to `lines`. */
  times: number[];
  busy: boolean;
  ready: boolean;
  where?: string;
  /** What the log is of: a launch (steps read from the output) or an operation on the built lab. */
  operation?: Operation;
  /** When the run began: its durations count from there (else from its first line). */
  startedAt?: number;
  /** The step the backend reports, for a run with no log here (it runs in another window, or
   *  its worker's log is empty): shown in place of "Preparing…", as the sidebar shows it. */
  liveStep?: string | null;
}) {
  const timeline = useDeployTimeline(lines, times, busy, operation, startedAt);
  const [showLog, setShowLog] = useState(false);
  const toggleLog = useCallback(() => {
    setShowLog((v) => !v);
  }, []);
  const { failed, timed, start } = timeline;
  return (
    <div>
      <DeployHeader where={where} timeline={timeline} busy={busy} ready={ready} operation={operation} />
      <StepList timeline={timeline} busy={busy} liveStep={liveStep} />
      {failed && (
        <p className="flex items-start gap-2 border-t border-border px-4 py-2.5 font-mono text-[0.6875rem] text-muted-foreground">
          <StatusDot tone="fail" className="mt-1 shrink-0" />
          <span className="min-w-0 break-words">{failed.line.replace(/^✗\s*/, "")}</span>
        </p>
      )}
      {timed.length > 0 && <RawLog timed={timed} origin={start ?? timed[0].at} shown={showLog} onToggle={toggleLog} />}
    </div>
  );
}
