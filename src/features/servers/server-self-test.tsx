"use client";

import { useEffect, useRef, useState } from "react";
import { Check, X } from "lucide-react";
import { StatusDot } from "@/components/ui/status-pill";
import { serverSelftest, type SelfTestEvent, type ServerHost } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { formatElapsed } from "@/lib/format";
import { useNow } from "@/lib/use-now";
import { useT, type MessageKey } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

/** One step the test runs, with its label. */
type PlanStep = { step: string; label: MessageKey };

/** The step plan each provider reports, shown up front so the list doesn't grow as it runs: the
 *  steps the core runs, no others (a step listed and never run reads as a test that stopped). */
export const SELFTEST_PLAN: Record<"proxmox" | "esxi", PlanStep[]> = {
  // A one-machine lab run the way a launch runs one (see the core's server self-test).
  proxmox: [
    { step: "connect", label: "servers.selfTest.steps.connect" },
    { step: "prepare", label: "servers.selfTest.steps.prepareLab" },
    { step: "image", label: "servers.selfTest.steps.image" },
    { step: "network", label: "servers.selfTest.steps.network" },
    { step: "vm", label: "servers.selfTest.steps.vm" },
    { step: "uplink", label: "servers.selfTest.steps.uplink" },
    { step: "ssh", label: "servers.selfTest.steps.ssh" },
    { step: "command", label: "servers.selfTest.steps.command" },
    { step: "cleanup", label: "servers.selfTest.steps.cleanupLab" },
  ],
  // Connection-only for now: the host answers, and its SSH port.
  esxi: [
    { step: "connect", label: "servers.selfTest.steps.connect" },
    { step: "ssh", label: "servers.selfTest.steps.esxiSsh" },
  ],
};

/** Each step's last state and detail, by step name (none for a step not reached). */
type StepStates = Partial<Record<string, { state: string; detail?: string }>>;

/** How a test ended, by its plan: passed only when every listed step passed. A step that failed
 *  (listed or not) fails it; a listed step that never passed, with none failed, leaves it
 *  incomplete, stopped before that step, never passed. */
export type SelfTestOutcome = { kind: "running" } | { kind: "passed" } | { kind: "failed" } | { kind: "incomplete"; before: PlanStep; why: string | null };

export function selfTestOutcome(plan: PlanStep[], states: StepStates, running: boolean, error: string | null): SelfTestOutcome {
  if (running) return { kind: "running" };
  if (Object.values(states).some((s) => s?.state === "fail")) return { kind: "failed" };
  const before = plan.find((p) => states[p.step]?.state !== "ok");
  if (before) return { kind: "incomplete", before, why: error };
  return error ? { kind: "failed" } : { kind: "passed" };
}

/** Each step's last state, from the events in the order they came. */
function statesOf(events: SelfTestEvent[]): StepStates {
  const states: StepStates = {};
  for (const e of events) states[e.step] = { state: e.state, detail: e.detail ?? undefined };
  return states;
}

/** A self-test in progress on a host, shared by every view of it: the test runs on the server
 *  whether or not its panel is shown, so a panel hidden and reopened while it runs attaches to
 *  it instead of starting a second throwaway VM beside the first. */
type Run = { startAt: number; events: SelfTestEvent[]; error: string | null; listeners: Set<(e: SelfTestEvent) => void>; onSettled: Set<() => void> };
const runs = new Map<string, Run>();

/** The run on host `id`, started now when none is in progress. */
function runFor(id: string): Run {
  const found = runs.get(id);
  if (found) return found;
  const run: Run = { startAt: Date.now(), events: [], error: null, listeners: new Set(), onSettled: new Set() };
  runs.set(id, run);
  serverSelftest(id, (e: SelfTestEvent) => {
    run.events.push(e);
    run.listeners.forEach((l) => {
      l(e);
    });
  })
    .catch((err: unknown) => {
      run.error = describeError(err);
    })
    .finally(() => {
      // Over: the next open starts a new one. A view still showing it hears the end.
      runs.delete(id);
      run.onSettled.forEach((f) => {
        f();
      });
    });
  return run;
}

/** The view's side of host `id`'s run: its steps so far, whether it still runs, why it couldn't
 *  run, and when it started. `onDone` hears how it ended. */
function useServerSelfTest(id: string, plan: PlanStep[], onDone: (result: "ok" | "fail") => void) {
  const [states, setStates] = useState<StepStates>({});
  const [running, setRunning] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [startAt, setStartAt] = useState(() => Date.now());
  const doneRef = useRef(onDone);
  useEffect(() => {
    doneRef.current = onDone;
  });

  useEffect(() => {
    const run = runFor(id);
    const shown = (e: SelfTestEvent) => {
      setStates((s) => ({ ...s, [e.step]: { state: e.state, detail: e.detail ?? undefined } }));
    };
    // Attaching to a run already going: what it did so far, then the rest as it comes.
    // eslint-disable-next-line react-hooks/set-state-in-effect -- the start lives in the module's run, which only this effect starts or finds
    setStartAt(run.startAt);
    run.events.forEach(shown);
    run.listeners.add(shown);
    const settled = () => {
      setRunning(false);
      if (run.error) setError(run.error);
      doneRef.current(selfTestOutcome(plan, statesOf(run.events), false, run.error).kind === "passed" ? "ok" : "fail");
    };
    run.onSettled.add(settled);
    return () => {
      // The view goes, the test on the server doesn't: a later view attaches to it.
      run.listeners.delete(shown);
      run.onSettled.delete(settled);
    };
    // The plan is the provider's constant.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  return { states, running, error, startAt };
}

/** Runs a real-VM self-test on a host and renders each step, like the machine VM test. It
 *  really provisions a throwaway VM on the server and destroys it, so it takes a few minutes. */
export function ServerSelfTest({ id, provider, onDone }: { id: string; provider: ServerHost["provider"]; onDone: (result: "ok" | "fail") => void }) {
  const t = useT();
  const plan = SELFTEST_PLAN[provider === "proxmox" ? "proxmox" : "esxi"];
  const { states, running, error, startAt } = useServerSelfTest(id, plan, onDone);
  const [now] = useNow(500, running);
  const outcome = selfTestOutcome(plan, states, running, error);
  const tone = outcome.kind === "running" ? "warn" : outcome.kind === "passed" ? "ok" : "fail";
  const headline =
    outcome.kind === "running"
      ? t("servers.selfTest.running")
      : outcome.kind === "passed"
        ? t("servers.selfTest.passed")
        : outcome.kind === "incomplete"
          ? t("servers.selfTest.incomplete", { step: t(outcome.before.label) })
          : t("servers.selfTest.failed");
  // Stopped with nothing failed and no error: say so, never let it pass as done.
  const shownError = outcome.kind === "incomplete" && !error ? t("servers.selfTest.notRun") : error;

  return (
    <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
      <div className="flex items-center gap-2.5 border-b border-border px-4 py-2">
        <StatusDot tone={tone} pulse={running} />
        <span className="text-[0.8125rem] font-medium">{headline}</span>
        <span className="ml-auto font-mono text-[0.6875rem] tabular-nums text-faint">{formatElapsed(now - startAt)}</span>
      </div>
      <StepList plan={plan} states={states} />
      {shownError && (
        <div className="flex items-start gap-2.5 border-t border-border px-4 py-2 text-[0.75rem] text-muted-foreground">
          <StatusDot tone="fail" className="mt-1.5" />
          <span className="min-w-0 break-words">{shownError}</span>
        </div>
      )}
    </div>
  );
}

/** The provider's steps, each with its mark and its last detail. */
function StepList({ plan, states }: { plan: { step: string; label: MessageKey }[]; states: StepStates }) {
  const t = useT();
  return (
    <div className="grid py-1">
      {plan.map((p) => {
        const st = states[p.step]?.state ?? "idle";
        const detail = states[p.step]?.detail;
        return (
          <div key={p.step} className="grid grid-cols-[1.25rem_minmax(0,1fr)] items-start gap-3 px-4 py-1.5 text-[0.8125rem]">
            <StepMark state={st} />
            <div className="min-w-0">
              <p className={cn(st === "idle" || st === "skip" ? "text-faint" : "text-foreground")}>{t(p.label)}</p>
              {detail && <p className="mt-0.5 break-words font-mono text-[0.6875rem] text-faint">{detail}</p>}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** The step's circle: filled with a check when done, a pulsing ring while it runs. */
function StepMark({ state }: { state: string }) {
  const base = "mt-px grid size-[1.1rem] place-items-center rounded-full";
  if (state === "ok")
    return (
      <span className={cn(base, "bg-success text-background")}>
        <Check className="size-2.5" strokeWidth={3.5} />
      </span>
    );
  if (state === "fail")
    return (
      <span className={cn(base, "bg-destructive text-background")}>
        <X className="size-2.5" strokeWidth={3.5} />
      </span>
    );
  if (state === "running")
    return (
      <span className={cn(base, "shadow-[inset_0_0_0_0.1rem_var(--warning)]")}>
        <StatusDot tone="warn" pulse />
      </span>
    );
  return <span className={cn(base, "shadow-[inset_0_0_0_0.1rem_var(--input)]", state === "skip" && "opacity-50")} />;
}
