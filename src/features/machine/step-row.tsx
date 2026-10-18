"use client";

import { type ReactNode } from "react";
import { Check, X } from "lucide-react";
import { cn } from "@/lib/utils";

// The step list of the guided setup, the self-tests and deploy progress (the mockup's .steps).

export type StepState = "done" | "run" | "fail" | "pending" | "skip";

/** The step marker: done = success disc with a check, run = warning ring with a pulse,
 *  fail = fail disc with a cross, pending (and skip) = an input ring. */
export function StepCircle({ state }: { state: StepState }) {
  return (
    <span
      aria-hidden
      className={cn(
        "grid size-[1.1rem] shrink-0 place-items-center rounded-full",
        state === "done"
          ? "bg-success text-background"
          : state === "fail"
            ? "bg-destructive text-background"
            : state === "run"
              ? "shadow-[inset_0_0_0_1.5px_var(--warning)]"
              : "shadow-[inset_0_0_0_1.5px_var(--input)]",
      )}
    >
      {state === "done" && <Check className="size-2.5" strokeWidth={3.5} />}
      {state === "fail" && <X className="size-2.5" strokeWidth={3.5} />}
      {state === "run" && <span className="dot-pulse size-1.5 rounded-full bg-warning" />}
    </span>
  );
}

/** One line of a step list: marker, label (with an optional mono detail under it), mono meta
 *  on the right, then an optional xs action. */
export function StepRow({
  state,
  label,
  detail,
  meta,
  action,
  detailTone,
}: {
  state: StepState;
  label: ReactNode;
  detail?: ReactNode;
  meta?: ReactNode;
  action?: ReactNode;
  detailTone?: "fail";
}) {
  return (
    <div className="grid grid-cols-[1.25rem_minmax(0,1fr)_auto] items-center gap-3 px-4 py-[0.55rem] text-[0.8125rem]">
      <StepCircle state={state} />
      <span className="min-w-0">
        <span className={cn("block", state === "pending" || state === "skip" ? "text-muted-foreground" : "text-foreground")}>{label}</span>
        {detail && (
          <span className={cn("block font-mono text-[0.6875rem] break-words", detailTone === "fail" ? "text-destructive" : "text-faint")}>{detail}</span>
        )}
      </span>
      {(meta || action) && (
        <span className="flex shrink-0 items-center gap-2.5">
          {meta && <span className="font-mono text-[0.6875rem] tabular-nums text-faint">{meta}</span>}
          {action}
        </span>
      )}
    </div>
  );
}
