import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type Tone = "ok" | "warn" | "fail" | "muted";

const DOT: Record<Tone, string> = { ok: "dot-ok", warn: "dot-warn", fail: "dot-fail", muted: "dot-idle" };

/** A status dot: ok (with a halo), warn, fail, idle. `pulse` for something in progress. */
export function StatusDot({ tone, pulse, className }: { tone: Tone; pulse?: boolean; className?: string }) {
  return <span aria-hidden className={cn("dot", DOT[tone], pulse && "dot-pulse", className)} />;
}

/** A status dot and its label (ready, needs attention, failed, idle). */
export function StatusPill({ tone, children, pulse }: { tone: Tone; children: ReactNode; pulse?: boolean }) {
  const c = { ok: "text-foreground", warn: "text-warning", fail: "text-destructive", muted: "text-muted-foreground" }[tone];
  return (
    <span className={cn("inline-flex items-center gap-2 text-[0.75rem] font-medium", c)}>
      <StatusDot tone={tone} pulse={pulse} />
      {children}
    </span>
  );
}
