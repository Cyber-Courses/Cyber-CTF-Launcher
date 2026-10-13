"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { machineSelftest, type SelfTestEvent } from "@/lib/tauri";
import { getVmProvider, setLastTest } from "@/lib/settings";
import { useNow } from "@/lib/use-now";

export type SelfTestResult = "idle" | "running" | "ok" | "fail";

/** When something started and ended (ms since epoch). */
export type Span = { start: number; end?: number };

type Kind = "docker" | "vm";

/** One kind's latest run, shared by every view of it: a view hidden and shown again, or a setup
 *  step left and come back to, shows the same run instead of starting a second one (for the VM
 *  test, another VM boot of minutes). */
type Shared = {
  result: SelfTestResult;
  events: Record<string, SelfTestEvent>;
  times: Record<string, Span>;
  run: Span | null;
  listeners: Set<() => void>;
};
const shared: Record<Kind, Shared> = {
  docker: { result: "idle", events: {}, times: {}, run: null, listeners: new Set() },
  vm: { result: "idle", events: {}, times: {}, run: null, listeners: new Set() },
};
const changed = (s: Shared) => s.listeners.forEach((l) => l());

/** The store's writes, outside any component (its state belongs to the app, not one view). */
function setRunResult(kind: Kind, r: SelfTestResult) {
  const s = shared[kind];
  if (r !== "running" && s.run) s.run = { ...s.run, end: Date.now() };
  s.result = r;
  changed(s);
}
function beginRun(kind: Kind) {
  Object.assign(shared[kind], { events: {}, times: {}, run: { start: Date.now() } });
}
function addEvent(kind: Kind, e: SelfTestEvent) {
  const s = shared[kind];
  const at = Date.now();
  const t = s.times[e.step] ?? { start: at };
  s.events = { ...s.events, [e.step]: e };
  s.times = { ...s.times, [e.step]: e.state === "running" ? t : { ...t, end: t.end ?? at } };
  changed(s);
}

/** Forgets the runs (tests start from a clean slate). */
export function resetSelfTests() {
  for (const s of Object.values(shared)) Object.assign(s, { result: "idle", events: {}, times: {}, run: null });
}

/** Runs a setup self-test and follows it: the last event and the timing of each step, the run's
 *  timing and its result (also saved as the last test). Runs on mount when `auto`, unless a run
 *  of this kind is already going (it then shows that one). */
export function useSelfTest(kind: Kind, auto: boolean | undefined, onResult: ((r: SelfTestResult) => void) | undefined) {
  const s = shared[kind];
  const [, rerender] = useState(0);
  useEffect(() => {
    const l = () => rerender((n) => n + 1);
    s.listeners.add(l);
    return () => void s.listeners.delete(l);
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
      setRunResult(kind, r);
      onResultRef.current?.(r);
    },
    [kind],
  );

  const runTest = useCallback(async () => {
    if (shared[kind].result === "running") return;
    beginRun(kind);
    touchNow();
    report("running");
    try {
      await machineSelftest(kind, kind === "vm" ? getVmProvider() : null, (e) => addEvent(kind, e));
      setLastTest(kind, "ok");
      report("ok");
    } catch {
      setLastTest(kind, "fail");
      report("fail");
    }
  }, [kind, report, touchNow]);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    // A run already going (the view was hidden mid-run) is shown, not started again.
    if (s.result === "running") onResultRef.current?.("running");
    else if (auto) void runTest();
  }, [auto, runTest, s]);

  return { events, result, run, times, now, runTest };
}
