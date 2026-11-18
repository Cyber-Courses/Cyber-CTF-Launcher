"use client";

import { useCallback, useMemo, useState } from "react";
import { PanelHeader } from "@/components/ui/panel";
import { StatusDot, StatusPill } from "@/components/ui/status-pill";
import { formatDuration } from "@/lib/format";
import type { ActiveOperation } from "@/lib/tauri";
import { type Timed } from "@/features/labs/deploy-log";
import type { SaidStep } from "@/lib/tauri/run-events";
import { type OperationText, spanOf, stateOf, stepsOf } from "@/features/labs/deploy-timeline";
import { LiveDuration, RawLog, StepItem, StepMark, AfterReadyStep } from "@/features/labs/deploy-step-parts";
import { useT, type T } from "@/lib/i18n";
import { failureText, isDoneLine, isFailedLine } from "@/lib/log-marks";

type Operation = ActiveOperation["op"];

const NO_MARKS: readonly SaidStep[] = [];

/** What a run's timeline is read from. */
type TimelineInput = { lines: string[]; times: number[]; marks: readonly SaidStep[]; busy: boolean; operation: Operation; startedAt: number | undefined };

// Operations on a lab that is already built: one step named after them, not the launch's
// "Building" / "Preparing", which read as a new deploy while the lab was shutting down.
const OPERATIONS = ["resume", "pause", "shutdown", "stop"] as const;

/** An operation's words (busy, its one step, done), when it is one of OPERATIONS. */
function operationText(t: T, operation: Operation): OperationText | undefined {
  const op = OPERATIONS.find((o) => o === operation);
  if (!op) return undefined;
  return { busy: t(`labs.deploy.ops.${op}.busy`), step: t(`labs.deploy.ops.${op}.step`), done: t(`labs.deploy.ops.${op}.done`) };
}

/**
 * The run read from its log: its timed lines (each line's arrival time comes with it: the deploy
 * store stamps its own lines, `useOperationLog` one the runner service ran), the ✓ or ✗ line that ended it, its
 * span (`end` null while it still runs: the clock ticks in `LiveDuration` alone), and its steps
 * (the ones the runner said, `marks`, or one step named after an operation on the built lab),
 * with the one that failed. Derived again only when the log changes, not on every tick of the
 * clock.
 */
function useDeployTimeline({ lines, times, marks, busy, operation, startedAt }: TimelineInput) {
  const t = useT();
  const timed = useMemo<Timed[]>(() => lines.map((line, i) => ({ line, at: times.at(i) ?? times.at(-1) ?? 0 })), [lines, times]);
  const derived = useMemo(() => {
    const failed = timed.find((row) => isFailedLine(row.line));
    const done = timed.find((row) => isDoneLine(row.line));
    const op = operationText(t, operation);
    const steps = stepsOf(t, timed, marks, { done, failed, op, operation });
    // From when the run began, when known: a run that failed on its first line otherwise read 0.0s.
    const first = timed.at(0)?.at;
    const start = startedAt !== undefined ? Math.min(startedAt, first ?? startedAt) : first;
    return { timed, failed, done, start, op, ...steps };
  }, [t, timed, marks, operation, startedAt]);
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

/** A launch's or a provisioning run's word while it runs: the one word each has everywhere. */
const busyWord = (t: T, operation: Operation) => t(operation === "provision" ? "common.operation.status.provision" : "common.operation.status.launch");

/** The steps, then Ready once done; before the first step shows, the step the backend reports
 *  (a run with no log here) or a pending Preparing row. */
function StepList({ timeline: tl, busy, liveStep }: { timeline: ReturnType<typeof useDeployTimeline>; busy: boolean; liveStep?: string | null }) {
  const t = useT();
  const [open, setOpen] = useState<string | null>(null);
  const { steps, done, op } = tl;
  return (
    <ul className="py-1 empty:hidden">
      {steps.map((p, i) => {
        // Once Ready has come, the start's steps are done, however long the operation takes to
        // report itself so; what runs after Ready is its own step below.
        const state = stateOf(p, tl, busy);
        // The first step from the run's start: what happened before its first line is its own.
        const { from, to } = spanOf(p, i, tl, state);
        const expanded = open === p.id;
        return (
          <StepItem
            key={p.id}
            step={p}
            state={state}
            from={from}
            to={to}
            origin={tl.start ?? from}
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
      {tl.after && <AfterReadyStep step={tl.after} failed={tl.failedStep === tl.after.id} busy={busy} origin={tl.start} open={open} setOpen={setOpen} />}
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

/** A lab's start-up as named steps with their durations, Vercel-build style (the steps the runner
 *  said, deploy-log.ts, each line filed under the one it came in). The raw log stays one click
 *  away. */
export function DeploySteps({
  lines,
  times,
  marks = NO_MARKS,
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
  /** The steps the runner said, each placed among the lines. */
  marks?: readonly SaidStep[];
  busy: boolean;
  ready: boolean;
  where?: string;
  /** What the log is of: a launch (steps read from the output) or an operation on the built lab. */
  operation?: Operation;
  /** When the run began: its durations count from there (else from its first line). */
  startedAt?: number;
  /** The step the backend reports, for a run with no log here (it runs in another window, or
   *  its operation's log is empty): shown in place of "Preparing…", as the sidebar shows it. */
  liveStep?: string | null;
}) {
  const timeline = useDeployTimeline({ lines, times, marks, busy, operation, startedAt });
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
          <span className="min-w-0 break-words">{failureText(failed.line)}</span>
        </p>
      )}
      {timed.length > 0 && <RawLog timed={timed} origin={start ?? timed[0].at} shown={showLog} onToggle={toggleLog} />}
    </div>
  );
}
