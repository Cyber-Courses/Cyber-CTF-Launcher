"use client";

import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { StepRow } from "@/features/machine/step-row";
import { useSelfTest, type SelfTestResult, type Span } from "@/features/machine/use-self-test";
import { type SelfTestEvent } from "@/lib/tauri";
import { formatElapsed } from "@/lib/format";
import { useT, type MessageKey } from "@/lib/i18n";

export type { SelfTestResult } from "@/features/machine/use-self-test";

/** The steps each test reports, so the list shows up front instead of growing as it runs. */
const PLAN: Record<"docker" | "vm", { step: string; label: MessageKey }[]> = {
  docker: [
    { step: "engine", label: "machine.selfTest.docker.engine" },
    { step: "pull", label: "machine.selfTest.docker.pull" },
    { step: "start", label: "machine.selfTest.docker.start" },
    { step: "network", label: "machine.selfTest.docker.network" },
    { step: "port", label: "machine.selfTest.docker.port" },
    { step: "cleanup", label: "machine.selfTest.docker.cleanup" },
  ],
  vm: [
    { step: "vagrant", label: "machine.selfTest.vm.vagrant" },
    { step: "provider", label: "machine.selfTest.vm.provider" },
    { step: "hypervisor", label: "machine.selfTest.vm.hypervisor" },
    { step: "box", label: "machine.selfTest.vm.box" },
    { step: "boot", label: "machine.selfTest.vm.boot" },
    { step: "exec", label: "machine.selfTest.vm.exec" },
    { step: "network", label: "machine.selfTest.vm.network" },
    { step: "cleanup", label: "machine.selfTest.vm.cleanup" },
  ],
};

export { resetSelfTests } from "@/features/machine/use-self-test";

/** A runnable setup self-test with a live checklist. Runs on mount when `auto`, unless a run of
 *  this kind is already going (it then shows that one). */
export function SelfTest({
  kind,
  title,
  description,
  auto,
  onResult,
}: {
  kind: "docker" | "vm";
  title: string;
  description: string;
  auto?: boolean;
  onResult?: (r: SelfTestResult) => void;
}) {
  const { events, result, run, times, now, runTest } = useSelfTest(kind, auto, onResult);
  const passed = PLAN[kind].filter(({ step }) => events[step]?.state === "ok").length;
  return (
    <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
      <div className="flex items-center gap-3 border-b border-border px-4 py-2.5">
        <div className="min-w-0">
          <p className="text-[0.8125rem] font-medium text-foreground">{title}</p>
          <p className="text-[0.75rem] text-muted-foreground">{description}</p>
        </div>
        <span className="ml-auto shrink-0">
          <SelfTestStatus result={result} passed={passed} total={PLAN[kind].length} run={run} now={now} onRun={runTest} />
        </span>
      </div>
      {result !== "idle" && (
        <ul className="py-1.5">
          {PLAN[kind].map(({ step, label }) => (
            <li key={step}>
              <SelfTestStep label={label} event={events[step]} time={times[step]} result={result} run={run} now={now} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Testing with progress and elapsed time, passed or failed with the run's time, and the button
 *  to run, retry or run again. */
function SelfTestStatus({
  result,
  passed,
  total,
  run,
  now,
  onRun,
}: {
  result: SelfTestResult;
  passed: number;
  total: number;
  run: Span | null;
  now: number;
  onRun: () => void;
}) {
  const t = useT();
  if (result === "running")
    return (
      <span className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
        <StatusDot tone="warn" pulse /> {t("machine.selfTest.testing")}
        <span className="font-mono text-[0.6875rem] tabular-nums text-faint">
          {passed}/{total} · {run ? formatElapsed(now - run.start) : ""}
        </span>
      </span>
    );
  if (result === "ok")
    return (
      <span className="flex items-center gap-2">
        <span className="flex items-center gap-2 text-[0.75rem] text-foreground">
          <StatusDot tone="ok" /> {t("machine.selfTest.passed")}
          {run?.end ? <span className="font-mono text-[0.6875rem] tabular-nums text-faint">{formatElapsed(run.end - run.start)}</span> : null}
        </span>
        <Button variant="ghost" size="icon-sm" onClick={onRun} aria-label={t("machine.selfTest.runAgain")}>
          <RefreshCw />
        </Button>
      </span>
    );
  return (
    <span className="flex items-center gap-2">
      {result === "fail" && (
        <span className="flex items-center gap-2 text-[0.75rem] text-destructive">
          <StatusDot tone="fail" /> {t("machine.selfTest.failed")}
          {run?.end && <span className="font-mono text-[0.6875rem] tabular-nums text-faint">{formatElapsed(run.end - run.start)}</span>}
        </span>
      )}
      <Button variant={result === "fail" ? "outline" : "primary"} size="xs" onClick={onRun}>
        {result === "fail" ? (
          <>
            <RefreshCw /> {t("machine.selfTest.retry")}
          </>
        ) : (
          t("machine.selfTest.run")
        )}
      </Button>
    </span>
  );
}

/** One step of the checklist: its state, detail and how long it took (still ticking while the
 *  run goes on), or "skipped". */
function SelfTestStep({
  label,
  event: e,
  time,
  result,
  run,
  now,
}: {
  label: MessageKey;
  event: SelfTestEvent | undefined;
  time: Span | undefined;
  result: SelfTestResult;
  run: Span | null;
  now: number;
}) {
  const t = useT();
  const state: SelfTestEvent["state"] | "pending" = e ? e.state : "pending";
  return (
    <StepRow
      state={state === "ok" ? "done" : state === "running" ? "run" : state}
      label={t(label)}
      detail={e?.detail}
      detailTone={state === "fail" ? "fail" : undefined}
      meta={
        time && state !== "skip"
          ? formatElapsed((time.end ?? (result === "running" ? now : (run?.end ?? now))) - time.start)
          : state === "skip"
            ? t("machine.selfTest.skipped")
            : undefined
      }
    />
  );
}
