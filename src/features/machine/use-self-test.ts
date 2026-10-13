"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { machineSelftest, type SelfTestEvent } from "@/lib/tauri";
import { getVmProvider, setLastTest } from "@/lib/settings";
import { useNow } from "@/lib/use-now";

export type SelfTestResult = "idle" | "running" | "ok" | "fail";

/** When something started and ended (ms since epoch). */
export type Span = { start: number; end?: number };

/** Runs a setup self-test and follows it: the last event and the timing of each step, the run's
 *  timing and its result (also saved as the last test). Runs once on mount when `auto`. */
export function useSelfTest(kind: "docker" | "vm", auto: boolean | undefined, onResult: ((r: SelfTestResult) => void) | undefined) {
  const [events, setEvents] = useState<Record<string, SelfTestEvent>>({});
  const [result, setResult] = useState<SelfTestResult>("idle");
  const started = useRef(false);
  // When the run and each step started and ended, for the counters.
  const [run, setRun] = useState<Span | null>(null);
  const [times, setTimes] = useState<Record<string, Span>>({});
  // Re-render once a second while running, so the counters tick.
  const [now, touchNow] = useNow(1000, result === "running");

  const report = useCallback(
    (r: SelfTestResult) => {
      setResult(r);
      onResult?.(r);
    },
    [onResult],
  );

  const runTest = useCallback(async () => {
    setEvents({});
    setTimes({});
    setRun({ start: Date.now() });
    touchNow();
    report("running");
    const onEvent = (e: SelfTestEvent) => {
      setEvents((prev) => ({ ...prev, [e.step]: e }));
      const at = Date.now();
      setTimes((prev) => {
        const t = prev[e.step] ?? { start: at };
        return { ...prev, [e.step]: e.state === "running" ? t : { ...t, end: t.end ?? at } };
      });
    };
    try {
      await machineSelftest(kind, kind === "vm" ? getVmProvider() : null, onEvent);
      setLastTest(kind, "ok");
      report("ok");
    } catch {
      setLastTest(kind, "fail");
      report("fail");
    } finally {
      setRun((r) => (r ? { ...r, end: Date.now() } : r));
    }
  }, [kind, report, touchNow]);

  useEffect(() => {
    if (auto && !started.current) {
      started.current = true;
      void runTest();
    }
  }, [auto, runTest]);

  return { events, result, run, times, now, runTest };
}
