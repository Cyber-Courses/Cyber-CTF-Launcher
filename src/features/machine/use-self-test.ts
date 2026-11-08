"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { machineSelftest, type Provider, type SelfTestEvent } from "@/lib/tauri";
import { getVmProvider, setLastTest } from "@/lib/settings";
import { useNow } from "@/lib/use-now";
import { registerReset } from "@/lib/module-state";

export type SelfTestResult = "idle" | "running" | "ok" | "fail";

/** When something started and ended (ms since epoch). */
export type Span = { start: number; end?: number };

type Kind = "docker" | "vm";

/** Which engine or hypervisor a test is of (the Machine page tests each of its rows), and how
 *  it is recorded. Without one, the test runs on what labs use (the setup window's). */
export type SelfTestTarget = {
  /** The provider the result is kept under: an engine id, or a hypervisor. */
  provider?: string | null;
  /** The Docker CLI context to run a container test on (one engine among several). */
  context?: string | null;
  /** Whether this is the engine or hypervisor labs run on: its result is the lab type's too. */
  primary?: boolean;
};

/** One run per kind and target, shared by every view of it: a view hidden and shown again, or a
 *  setup step left and come back to, shows the same run instead of starting a second one (for
 *  the VM test, another VM boot of minutes). */
type Shared = {
  result: SelfTestResult;
  /** Each step's last event, by step (none for a step not reached). */
  events: Partial<Record<string, SelfTestEvent>>;
  /** Each step's timing, by step. */
  times: Partial<Record<string, Span>>;
  run: Span | null;
  listeners: Set<() => void>;
};
const shared = new Map<string, Shared>();
const runKey = (kind: Kind, target: SelfTestTarget) => `${kind}:${target.provider ?? ""}`;
function store(key: string): Shared {
  let s = shared.get(key);
  if (!s) {
    s = { result: "idle", events: {}, times: {}, run: null, listeners: new Set() };
    shared.set(key, s);
  }
  return s;
}
const changed = (s: Shared) => {
  s.listeners.forEach((l) => {
    l();
  });
};

/** The store's writes, outside any component (its state belongs to the app, not one view). */
function setRunResult(s: Shared, r: SelfTestResult) {
  if (r !== "running" && s.run) s.run = { ...s.run, end: Date.now() };
  s.result = r;
  changed(s);
}
function beginRun(s: Shared) {
  Object.assign(s, { events: {}, times: {}, run: { start: Date.now() } });
}
function addEvent(s: Shared, e: SelfTestEvent) {
  const at = Date.now();
  const t = s.times[e.step] ?? { start: at };
  s.events = { ...s.events, [e.step]: e };
  s.times = { ...s.times, [e.step]: e.state === "running" ? t : { ...t, end: t.end ?? at } };
  changed(s);
}
/** Why a run failed: its failing step's detail, else that step's label. */
function failure(s: Shared): string | null {
  const failed = Object.values(s.events).find((e) => e?.state === "fail");
  return failed ? (failed.detail ?? failed.label) : null;
}

/** One run of the test on `target`, its events into `s`: the result, saved as the last test of
 *  the target's provider and, for the one labs run on, of the kind. */
async function perform(kind: Kind, s: Shared, { provider = null, context = null, primary = true }: SelfTestTarget): Promise<SelfTestResult> {
  // A VM test runs on the hypervisor asked for, else the one chosen in Settings.
  const hypervisor = kind === "vm" ? ((provider as Provider | null) ?? getVmProvider()) : null;
  try {
    await machineSelftest(
      kind,
      hypervisor,
      (e) => {
        addEvent(s, e);
      },
      kind === "docker" ? context : null,
    );
    setLastTest(kind, "ok", { provider, primary });
    return "ok";
  } catch {
    setLastTest(kind, "fail", { provider, primary, why: failure(s) });
    return "fail";
  }
}

// Forgets the runs (see registerReset).
registerReset(() => {
  for (const s of shared.values()) Object.assign(s, { result: "idle", events: {}, times: {}, run: null });
});

/** Runs a setup self-test and follows it: the last event and the timing of each step, the run's
 *  timing and its result (also saved as the last test, of the target's provider and, for the one
 *  labs run on, of the kind). Runs on mount when `auto`, unless a run of this kind and target is
 *  already going (it then shows that one). */
export function useSelfTest(kind: Kind, auto: boolean | undefined, onResult: ((r: SelfTestResult) => void) | undefined, target: SelfTestTarget = {}) {
  const { provider = null, context = null, primary = true } = target;
  const s = store(runKey(kind, target));
  const [, rerender] = useState(0);
  useEffect(() => {
    const l = () => {
      rerender((n) => n + 1);
    };
    s.listeners.add(l);
    return () => {
      s.listeners.delete(l);
    };
  }, [s]);
  const { events, result, run, times } = s;
  const started = useRef(false);
  // Re-render once a second while running, so the counters tick.
  const [now, touchNow] = useNow(1000, result === "running");

  const onResultRef = useRef(onResult);
  useEffect(() => {
    onResultRef.current = onResult;
  });
  const report = useCallback(
    (r: SelfTestResult) => {
      setRunResult(s, r);
      onResultRef.current?.(r);
    },
    [s],
  );

  const runTest = useCallback(async () => {
    if (s.result === "running") return;
    beginRun(s);
    touchNow();
    report("running");
    report(await perform(kind, s, { provider, context, primary }));
  }, [kind, provider, context, primary, report, s, touchNow]);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    // A run already going (the view was hidden mid-run) is shown, not started again.
    if (s.result === "running") onResultRef.current?.("running");
    else if (auto) void runTest();
  }, [auto, runTest, s]);

  return {
    events,
    result,
    run,
    times,
    now,
    runTest: () => {
      void runTest();
    },
  };
}
