"use client";

import { memo, useEffect, useRef } from "react";
import { Check, ChevronRight, X } from "lucide-react";
import { LogConsole } from "@/components/ui/log-console";
import { cn } from "@/lib/utils";
import { formatDuration } from "@/lib/format";
import type { Step, Timed } from "@/features/labs/deploy-log";
import { useNow } from "@/lib/use-now";
import { useT } from "@/lib/i18n";

// The parts of DeploySteps: a step's row and mark, its own lines, and the raw log.

export type StepState = "ok" | "running" | "fail" | "pending";

/** A step's 1.1rem circle: done (filled success with a check), running (warning ring with a
 *  pulsing dot), failed (fail ring with a cross), pending (an input ring). */
export function StepMark({ state }: { state: StepState }) {
  return (
    <span
      aria-hidden
      className={cn(
        "flex size-[1.1rem] items-center justify-center rounded-full",
        state === "ok" && "bg-success text-background",
        state === "running" && "shadow-[inset_0_0_0_0.09375rem_var(--warning)]",
        state === "fail" && "text-destructive shadow-[inset_0_0_0_0.09375rem_var(--destructive)]",
        state === "pending" && "shadow-[inset_0_0_0_0.09375rem_var(--input)]",
      )}
    >
      {state === "ok" && <Check className="size-2.5" strokeWidth={3.5} />}
      {state === "running" && <span className="size-[0.4rem] animate-pulse rounded-full bg-warning" />}
      {state === "fail" && <X className="size-2.5" strokeWidth={3} />}
    </span>
  );
}

/** A duration still running, from `from` to now: the one part of the timeline that ticks
 *  (every 100 ms), so the steps and the log around it don't re-render with it. */
export function LiveDuration({ from }: { from: number }) {
  const [now] = useNow(100);
  return <>{formatDuration(Math.max(0, now - from))}</>;
}

/** A phase's own lines, each with its time since the launch started. */
function TimedLog({ rows, origin }: { rows: Timed[]; origin: number }) {
  const box = useRef<HTMLDivElement>(null);
  // Tail the log: keep the newest line in view as the deployment streams.
  useEffect(() => {
    const el = box.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [rows]);
  return (
    <div ref={box} className="surface-log max-h-48 overflow-auto rounded-sm px-3 py-2 font-mono text-[0.6875rem] leading-relaxed">
      {rows.map((r, i) => (
        <div key={i} className="flex gap-3">
          <span className="w-12 shrink-0 text-right tabular-nums text-faint">{formatDuration(r.at - origin)}</span>
          <span className="min-w-0 break-words text-muted-foreground">{r.line}</span>
        </div>
      ))}
    </div>
  );
}

/** One step: its mark, label and duration (`from` to `to`), opening onto its own lines. */
export function StepItem({
  step: p,
  state,
  from,
  to,
  origin,
  expanded,
  onToggle,
}: {
  step: Step;
  state: StepState;
  from: number;
  /** When it ended; null while it runs (its duration then ticks). */
  to: number | null;
  /** When the run started, for the lines' times. */
  origin: number;
  expanded: boolean;
  onToggle: () => void;
}) {
  const rows = p.rows;
  const latest = rows.at(-1)?.line;
  return (
    <li>
      <button
        type="button"
        aria-expanded={expanded}
        onClick={onToggle}
        className="group grid w-full grid-cols-[1.25rem_minmax(0,1fr)_auto_0.875rem] items-center gap-3 px-4 py-2 text-left text-[0.8125rem] transition-colors hover:bg-glass"
      >
        <StepMark state={state} />
        <span className={cn("truncate", state === "fail" ? "text-destructive" : "text-foreground")}>{p.label}</span>
        <span className="font-mono text-[0.6875rem] tabular-nums text-faint">{to === null ? <LiveDuration from={from} /> : formatDuration(to - from)}</span>
        <ChevronRight className={cn("size-3.5 text-faint transition-transform group-hover:text-muted-foreground", expanded && "rotate-90")} />
      </button>
      {/* While a phase is running, show its latest line inline (e.g. which container is
          still "Waiting"), so the long "Wait until healthy" step isn't a blank spinner. */}
      {state === "running" && !expanded && latest && (
        <div className="-mt-1 truncate px-4 pb-2 pl-12 font-mono text-[0.6875rem] text-faint">{latest.trim()}</div>
      )}
      {expanded && (
        <div className="px-4 pb-2.5 pl-12">
          <TimedLog rows={rows} origin={origin} />
        </div>
      )}
    </li>
  );
}

/** The whole log, one click away, each line with its time since the start. Memoized: it changes
 *  only with the log. */
export const RawLog = memo(function RawLog({ timed, origin, shown, onToggle }: { timed: Timed[]; origin: number; shown: boolean; onToggle: () => void }) {
  const t = useT();
  return (
    <div className="border-t border-border px-4 py-2">
      <button
        type="button"
        aria-expanded={shown}
        onClick={onToggle}
        className="flex items-center gap-1.5 text-[0.75rem] text-muted-foreground transition-colors hover:text-foreground"
      >
        <ChevronRight className={cn("size-3.5 transition-transform", shown && "rotate-90")} /> {shown ? t("labs.deploy.hideLog") : t("labs.deploy.showLog")}
      </button>
      {shown && (
        <div className="mt-2 pb-1">
          <LogConsole lines={timed.map((row) => `${formatDuration(row.at - origin).padStart(6)}  ${row.line}`)} />
        </div>
      )}
    </div>
  );
});
