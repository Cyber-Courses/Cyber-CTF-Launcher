"use client";

import { useEffect, useRef, useState } from "react";
import { Check, X } from "lucide-react";
import { StatusDot } from "@/components/ui/status-pill";
import { SELFTEST_STEPS, serverSelftest, type SelfTestEvent, type ServerHost } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { formatBytes, formatElapsed } from "@/lib/format";
import { progressText } from "@/lib/progress-line";
import { useNow } from "@/lib/use-now";
import { useT, type MessageKey } from "@/lib/i18n";
import { describeError } from "@/lib/errors";
import { stepAfter, statesOf, stepTime, type Stamped, type StepStates } from "@/features/servers/self-test-steps";

/** One step the test runs, with its label; `uploads` when it uploads the VM's image (it then
 *  says up front that this takes minutes). */
type PlanStep = { step: string; label: MessageKey; uploads?: true };

/** The steps `steps` (the core's, in its order), each with its label in `labels`, which names
 *  each of them and no other; the step `uploads` names uploads the VM's image. */
const planOf = <S extends string>(steps: readonly S[], labels: Record<NoInfer<S>, MessageKey>, uploads?: NoInfer<S>): PlanStep[] =>
  steps.map((step) => (step === uploads ? { step, label: labels[step], uploads: true } : { step, label: labels[step] }));

/** The step plan each provider reports, shown up front so the list doesn't grow as it runs: the
 *  steps the core runs, no others (a step listed and never run reads as a test that stopped). */
export const SELFTEST_PLAN: Record<"proxmox" | "esxi", PlanStep[]> = {
  // A one-machine lab run the way a launch runs one (see the core's server self-test).
  proxmox: planOf(SELFTEST_STEPS.proxmox, {
    connect: "servers.selfTest.steps.connect",
    prepare: "servers.selfTest.steps.prepareLab",
    image: "servers.selfTest.steps.image",
    network: "servers.selfTest.steps.network",
    vm: "servers.selfTest.steps.vm",
    uplink: "servers.selfTest.steps.uplink",
    ssh: "servers.selfTest.steps.ssh",
    command: "servers.selfTest.steps.command",
    cleanup: "servers.selfTest.steps.cleanupLab",
  }),
  // The same lab run the way a launch runs one on ESXi: Vagrant's ESXi plugin, the OVF Tool's upload.
  esxi: planOf(
    SELFTEST_STEPS.vmware_esxi,
    {
      connect: "servers.selfTest.steps.connect",
      prepare: "servers.selfTest.steps.prepareLab",
      image: "servers.selfTest.steps.esxiImage",
      vm: "servers.selfTest.steps.esxiVm",
      address: "servers.selfTest.steps.esxiAddress",
      ssh: "servers.selfTest.steps.ssh",
      command: "servers.selfTest.steps.command",
      cleanup: "servers.selfTest.steps.cleanupLab",
    },
    "image",
  ),
};

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

/** A self-test in progress on a host, shared by every view of it: the test runs on the server
 *  whether or not its panel is shown, so a panel hidden and reopened while it runs attaches to
 *  it instead of starting a second throwaway VM beside the first. */
type Run = { startAt: number; events: Stamped[]; error: string | null; listeners: Set<(e: Stamped) => void>; onSettled: Set<() => void> };
const runs = new Map<string, Run>();

/** The run on host `id`, started now when none is in progress. */
function runFor(id: string): Run {
  const found = runs.get(id);
  if (found) return found;
  const run: Run = { startAt: Date.now(), events: [], error: null, listeners: new Set(), onSettled: new Set() };
  runs.set(id, run);
  serverSelftest(id, (e: SelfTestEvent) => {
    const stamped = { e, at: Date.now() };
    run.events.push(stamped);
    run.listeners.forEach((l) => {
      l(stamped);
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
  const planRef = useRef(plan);
  useEffect(() => {
    doneRef.current = onDone;
    planRef.current = plan;
  });

  useEffect(() => {
    const run = runFor(id);
    const shown = (stamped: Stamped) => {
      setStates((s) => ({ ...s, [stamped.e.step]: stepAfter(s[stamped.e.step], stamped) }));
    };
    // Attaching to a run already going: what it did so far, then the rest as it comes.
    // eslint-disable-next-line react-hooks/set-state-in-effect -- the start lives in the module's run, which only this effect starts or finds
    setStartAt(run.startAt);
    run.events.forEach(shown);
    run.listeners.add(shown);
    const settled = () => {
      setRunning(false);
      if (run.error) setError(run.error);
      doneRef.current(selfTestOutcome(planRef.current, statesOf(run.events), false, run.error).kind === "passed" ? "ok" : "fail");
    };
    run.onSettled.add(settled);
    return () => {
      // The view goes, the test on the server doesn't: a later view attaches to it.
      run.listeners.delete(shown);
      run.onSettled.delete(settled);
    };
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
      <StepList plan={plan} states={states} now={now} />
      {shownError && (
        <div className="flex items-start gap-2.5 border-t border-border px-4 py-2 text-[0.75rem] text-muted-foreground">
          <StatusDot tone="fail" className="mt-1.5" />
          <span className="min-w-0 break-words">{shownError}</span>
        </div>
      )}
    </div>
  );
}

/** The provider's steps, each with its mark, its last detail (an upload's progress as such) and
 *  how long it ran; the step that uploads the VM's image says up front that it takes minutes. */
function StepList({ plan, states, now }: { plan: PlanStep[]; states: StepStates; now: number }) {
  const t = useT();
  return (
    <div className="grid py-1">
      {plan.map((p) => {
        const s = states[p.step];
        const st = s?.state ?? "idle";
        const detail = s?.detail && (st === "running" ? progressText(s.detail) : s.detail);
        const time = stepTime(s, now);
        return (
          <div key={p.step} className="grid grid-cols-[1.25rem_minmax(0,1fr)_auto] items-start gap-3 px-4 py-1.5 text-[0.8125rem]">
            <StepMark state={st} />
            <div className="min-w-0">
              <p className={cn(st === "idle" || st === "skip" ? "text-faint" : "text-foreground")}>{t(p.label)}</p>
              {p.uploads && st === "running" && (
                <p className="mt-0.5 break-words text-[0.75rem] text-muted-foreground">
                  {s?.size ? t("servers.selfTest.uploadHintSize", { size: formatBytes(s.size) }) : t("servers.selfTest.uploadHint")}
                </p>
              )}
              {detail && <p className="mt-0.5 break-words font-mono text-[0.6875rem] text-faint">{detail}</p>}
            </div>
            <span className="mt-px font-mono text-[0.6875rem] tabular-nums text-faint">{time}</span>
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
