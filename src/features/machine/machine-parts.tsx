"use client";

import { type ReactNode } from "react";
import { Panel } from "@/components/ui/panel";
import { Meter } from "@/components/ui/meter";
import { Sparkline } from "@/components/ui/sparkline";
import { Skeleton } from "@/components/ui/skeleton";
import { SelfTest } from "@/features/machine/self-test";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { StatusDot, type Tone } from "@/components/ui/status-pill";

// ---------- small parts ----------

/** Over this, a usage figure reads as a warning (sparkline in the warning colour). */
const WARN_PCT = 75;

/**
 * A stat card: label and mono detail on top, a serif value with a small % unit, then a
 * sparkline of the recent values (or a meter when there is no history to draw).
 */
export function StatCard({
  label,
  detail,
  value,
  history,
  meter,
  unavailable = false,
}: {
  label: string;
  detail: string;
  value: number | null;
  history?: number[];
  meter?: boolean;
  /** No value, and none is coming (reading it failed): a dash, not placeholders. */
  unavailable?: boolean;
}) {
  return (
    <Panel className="grid gap-2.5 px-[1.1rem] py-4">
      <div className="flex items-baseline justify-between gap-3 text-[0.8125rem] text-muted-foreground">
        <span>{label}</span>
        <span className="truncate font-mono text-[0.6875rem] text-faint">{detail}</span>
      </div>
      {value === null && unavailable ? (
        <div className="serif-title text-[2rem] leading-none text-faint" aria-hidden>
          –
        </div>
      ) : value === null ? (
        <>
          <Skeleton className="h-8 w-20" />
          <Skeleton className={meter ? "h-1.5 w-full" : "h-9 w-full"} />
        </>
      ) : (
        <>
          <div className="serif-title text-[2rem] leading-none tabular-nums">
            {Math.round(value)}
            <small className="ml-1 font-sans text-[0.8125rem] tracking-normal text-faint">%</small>
          </div>
          {meter ? <Meter value={value} className="mt-1" /> : <Sparkline values={history?.length ? history : [value]} warn={value > WARN_PCT} />}
        </>
      )}
    </Panel>
  );
}

/** A compact callout row for warnings and errors: an icon or dot in the status colour, the
 *  text in foreground and muted, and small actions on the right. */
export function CalloutRow({
  tone,
  icon,
  title,
  meta,
  children,
  actions,
  className,
}: {
  tone: "warn" | "fail";
  icon?: ReactNode;
  title: ReactNode;
  meta?: ReactNode;
  children?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <Panel className={className}>
      <div role="alert" className="flex flex-wrap items-center gap-3 px-4 py-3">
        <span className={cn("flex size-4 shrink-0 items-center justify-center", tone === "warn" ? "text-warning" : "text-destructive")}>
          {icon ?? <StatusDot tone={tone} />}
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[0.8125rem] font-medium text-foreground">
            {title}
            {meta && <span className="ml-2 font-mono text-[0.6875rem] font-normal text-faint">{meta}</span>}
          </p>
          {children && <p className="mt-0.5 text-[0.75rem] text-muted-foreground">{children}</p>}
        </div>
        {actions && <div className="flex shrink-0 gap-2">{actions}</div>}
      </div>
    </Panel>
  );
}

/** A read-only tool row for the Details section. */
export function DetailRow({ name, value, bad }: { name: string; value: string; bad?: boolean }) {
  return (
    <div className="flex items-center gap-2.5 border-t border-border px-4 py-2.5 text-[0.8125rem] first:border-t-0">
      <span className="text-muted-foreground">{name}</span>
      <span className={cn("ml-auto font-mono text-[0.75rem]", bad ? "text-warning" : "text-foreground")}>{value}</span>
    </div>
  );
}

export function ListSkeleton() {
  return (
    <div className="space-y-2 px-4 py-3.5">
      <Skeleton className="h-4 w-2/3" />
      <Skeleton className="h-4 w-1/2" />
    </div>
  );
}

// ---------- lab types ----------

export type LabKind = "docker" | "vm";

const STATUS_TEXT: Record<Tone, string> = {
  ok: "text-foreground",
  warn: "text-warning",
  fail: "text-destructive",
  muted: "text-muted-foreground",
};

/** One lab type as a provider row: logo tile, name over mono detail, status (dot + word) and
 *  the one action that matters on the right; the self-test opens inline below it. */
export function LabTypeRow({
  kind,
  icon,
  title,
  tone,
  status,
  detail,
  hint,
  actions,
  testing,
  onTestDone,
}: {
  kind: LabKind;
  icon: ReactNode;
  title: string;
  tone: Tone;
  status: string;
  detail: ReactNode;
  hint?: string;
  actions: ReactNode;
  testing: boolean;
  onTestDone: () => void;
}) {
  const t = useT();
  return (
    <div className="border-t border-border first:border-t-0">
      <div className="flex flex-wrap items-center gap-x-3.5 gap-y-2 px-4 py-3">
        {icon}
        <div className="min-w-0 flex-1">
          <p className="text-[0.8125rem] font-medium text-foreground">{title}</p>
          <p className="mt-0.5 font-mono text-[0.6875rem] text-faint">{detail}</p>
          {hint && <p className="mt-1 text-[0.75rem] text-warning">{hint}</p>}
        </div>
        <span className={cn("inline-flex shrink-0 items-center gap-2 text-[0.75rem]", STATUS_TEXT[tone])}>
          <StatusDot tone={tone} />
          {status}
        </span>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
      {testing && (
        <div className="px-4 pb-4">
          <SelfTest
            kind={kind}
            title={kind === "docker" ? t("machine.selfTest.containerTitle") : t("machine.selfTest.vmTitle")}
            description={kind === "docker" ? t("machine.selfTest.machineDockerDescription") : t("machine.selfTest.machineVmDescription")}
            auto
            onResult={(r) => {
              if (r === "ok" || r === "fail") onTestDone();
            }}
          />
        </div>
      )}
    </div>
  );
}
