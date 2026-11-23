"use client";

import { useState } from "react";
import { machineMetrics, type MachineMetrics } from "@/lib/tauri";
import { usePoll } from "@/lib/use-poll";
import { ignore } from "@/lib/failure";
import { appendUsage, type UsageHistory } from "@/features/machine/machine-status";

/**
 * Live machine usage, polled `ms` after the last read ended, with a rolling history of each
 * figure for the stat card sparklines. One read at a time: a read asks Docker how many containers
 * run, which a Docker Desktop still starting answers only after its own wait (about 47 s, then a
 * 500), so reads every 3 s from each one's start stacked a `docker ps` each, eight at a time on
 * the Overview, and a killed app left them behind.
 */
export function useMachineMetrics(ms: number) {
  const [metrics, setMetrics] = useState<MachineMetrics | null>(null);
  const [history, setHistory] = useState<UsageHistory>({ cpu: [], mem: [], disk: [] });
  // The last read failed (the next one may still answer).
  const [failed, setFailed] = useState(false);
  usePoll(machineMetrics, ms, {
    onValue: (m) => {
      setMetrics(m);
      setHistory((h) => appendUsage(h, m));
      setFailed(false);
    },
    // Polled every few seconds: said quietly, as every poll's failure is, and shown as unavailable.
    onError: (e) => {
      ignore("the usage figures read as unavailable; the next read may answer")(e);
      setFailed(true);
    },
  });
  return { metrics, history, failed };
}
