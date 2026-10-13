"use client";

import { useEffect, useState } from "react";
import { PanelHeader } from "@/components/ui/panel";
import { StatusDot, StatusPill } from "@/components/ui/status-pill";
import { formatDuration } from "@/lib/format";
import type { ActiveOperation } from "@/lib/tauri";
import { deriveSteps, failedMachineStep, type Timed } from "@/features/labs/deploy-log";
import { RawLog, StepItem, StepMark } from "@/features/labs/deploy-step-parts";
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

/** Times each log line when it arrives; starts over when a new launch clears the log. */
function useTimedLines(lines: string[]): Timed[] {
  const [timed, setTimed] = useState<Timed[]>([]);
  useEffect(() => {
    const at = Date.now();
    // Records when lines arrived, a fact only known as they stream in.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setTimed((prev) => {
      const base = lines.length < prev.length || (prev.length > 0 && prev[0].line !== lines[0]) ? [] : prev;
      return base.length === lines.length ? base : [...base, ...lines.slice(base.length).map((line) => ({ line, at }))];
    });
  }, [lines]);
  return timed;
}

/** The current time, ticking every 100 ms while `busy`, for the running step's counter. */
function useTicking(busy: boolean) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!busy) return;
    const timer = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(timer);
  }, [busy]);
  return now;
}

/**
 * The run read from its log: its timed lines, the ✓ or ✗ line that ended it, its span, and its
 * steps (built from the log per the detected target, Vagrant / Terraform / Docker, or one step
 * named after an operation on the built lab), with the one that failed.
 */
function useDeployTimeline(lines: string[], times: number[] | undefined, busy: boolean, operation: Operation) {
  const t = useT();
  const fallback = useTimedLines(lines);
  // Prefer the per-line timestamps kept in the deploy store (they survive leaving and returning
  // to the page, so the step durations don't reset); fall back to local timing if absent.
  const timed: Timed[] = times && times.length === lines.length ? lines.map((line, i) => ({ line, at: times[i] })) : fallback;
  const now = useTicking(busy);
  const failed = timed.find((row) => row.line.startsWith("✗"));
  const done = timed.find((row) => row.line.startsWith("✓"));
  const start = timed[0]?.at;
  const end = done?.at ?? failed?.at ?? (busy ? now : timed.at(-1)?.at);
  const op = operationText(t, operation);
  const steps = op ? (timed.length > 0 ? [{ id: operation, label: op.step, rows: timed }] : []) : deriveSteps(t, timed);
  const lastSeen = steps.at(-1)?.id;
  const named = failed ? failedMachineStep(failed.line) : null;
  const failedStep = failed ? (named && steps.some((s) => s.id === named) ? named : lastSeen) : null;
  return { timed, now, failed, done, start, end, op, steps, lastSeen, failedStep };
}

/** The panel's header: where it deploys, its state (failed, building, ready, done) and how long. */
function DeployHeader({
  where,
  timeline: { failed, done, start, end, op, timed },
  busy,
  ready,
}: {
  where?: string;
  timeline: ReturnType<typeof useDeployTimeline>;
  busy: boolean;
  ready: boolean;
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
              {op?.busy ?? t("labs.deploy.building")}
            </StatusPill>
          ) : ready || (done && !op) ? (
            <StatusPill tone="ok">{t("labs.deploy.ready")}</StatusPill>
          ) : op && timed.length > 0 ? (
            <StatusPill tone="muted">{op.done}</StatusPill>
          ) : null}
          {start && end && <span className="font-mono text-[0.6875rem] tabular-nums text-faint">{formatDuration(end - start)}</span>}
        </span>
      }
    />
  );
}

/** The steps, then Ready once done, or a pending Preparing row before the first step shows. */
function StepList({ timeline: tl, busy }: { timeline: ReturnType<typeof useDeployTimeline>; busy: boolean }) {
  const t = useT();
  const [open, setOpen] = useState<string | null>(null);
  const { steps, now, done, failed, op } = tl;
  return (
    <ul className="py-1 empty:hidden">
      {steps.map((p, i) => {
        const next = steps[i + 1]?.rows[0]?.at;
        const state = p.id === tl.failedStep ? "fail" : p.id === tl.lastSeen && busy ? "running" : "ok";
        const to = next ?? (state === "running" ? now : (done?.at ?? failed?.at ?? p.rows.at(-1)!.at));
        const expanded = open === p.id;
        return (
          <StepItem
            key={p.id}
            step={p}
            state={state}
            from={p.rows[0].at}
            to={to}
            origin={tl.start!}
            expanded={expanded}
            onToggle={() => setOpen(expanded ? null : p.id)}
          />
        );
      })}
      {done && !op && (
        <li className="grid grid-cols-[1.25rem_minmax(0,1fr)] items-center gap-3 px-4 py-2 text-[0.8125rem]">
          <StepMark state="ok" />
          <span className="text-foreground">{t("labs.deploy.ready")}</span>
        </li>
      )}
      {busy && steps.length === 0 && (
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
}: {
  lines: string[];
  times?: number[];
  busy: boolean;
  ready: boolean;
  where?: string;
  /** What the log is of: a launch (steps read from the output) or an operation on the built lab. */
  operation?: Operation;
}) {
  const timeline = useDeployTimeline(lines, times, busy, operation);
  const [showLog, setShowLog] = useState(false);
  const { failed, timed, start } = timeline;
  return (
    <div>
      <DeployHeader where={where} timeline={timeline} busy={busy} ready={ready} />
      <StepList timeline={timeline} busy={busy} />
      {failed && (
        <p className="flex items-start gap-2 border-t border-border px-4 py-2.5 font-mono text-[0.6875rem] text-muted-foreground">
          <StatusDot tone="fail" className="mt-1 shrink-0" />
          <span className="min-w-0 break-words">{failed.line.replace(/^✗\s*/, "")}</span>
        </p>
      )}
      {timed.length > 0 && <RawLog timed={timed} origin={start!} shown={showLog} onToggle={() => setShowLog((v) => !v)} />}
    </div>
  );
}
