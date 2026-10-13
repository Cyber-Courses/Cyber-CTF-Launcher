"use client";

import { useEffect, useMemo, useState } from "react";
import { apiQuery, machineWorkloads, type ActiveOperation } from "@/lib/tauri";
import { useDeployingLabs } from "@/lib/deploy-store";
import { useLabState } from "@/features/labs/lab-store";
import { usePolled } from "@/lib/use-poll";
import { activeLabs, type SidebarLab } from "@/components/shell/shell-model";

const WORKLOADS_POLL_MS = 8000;

/**
 * The lab list behind the sidebar and the command palette (a light one, to jump straight to a
 * lab), loaded again on sign-in changes, and the labs running or starting right now, shown as
 * their own sidebar entries so one is a click away wherever you are. Deploys come from the
 * backend (so they survive a reload); the running set is polled from this machine's workloads.
 */
export function useSidebarLabs(loggedIn: boolean | undefined, ops: Map<string, ActiveOperation>) {
  const [labs, setLabs] = useState<SidebarLab[]>([]);
  useEffect(() => {
    // A sign-in change starts a new read: the one in flight from before it must not land last.
    let alive = true;
    apiQuery<{ labs: SidebarLab[] }>("sidebarLabs")
      .then((d) => alive && setLabs(d.labs))
      .catch(() => alive && setLabs([]));
    return () => {
      alive = false;
    };
  }, [loggedIn]);

  const deploying = useDeployingLabs();
  // Read again as soon as an operation starts or ends: a lab just shut down otherwise kept its
  // "Running" line until the next poll.
  const busyKey = [...deploying, ...ops.keys()].sort().join(",");
  const [runningIds] = usePolled(
    () => machineWorkloads().then((w) => new Set(w.map((x) => x.id).filter((id) => id !== "selftest"))),
    WORKLOADS_POLL_MS,
    new Set<string>(),
    { serial: true, restartKey: busyKey },
  );
  const statuses = useLabState("statuses");
  // When this light list couldn't be read (offline), the catalogue a screen loaded since (its
  // Retry) stands in, so the entries and the palette come back with it.
  const catalogue = useLabState("catalogue");
  const list = useMemo(() => (labs.length > 0 ? labs : (catalogue?.labs ?? labs)), [labs, catalogue]);

  // The same list object until one of its inputs changes, so the sidebar's entries don't re-render.
  const active = useMemo(() => activeLabs(list, deploying, ops, runningIds, statuses), [list, deploying, ops, runningIds, statuses]);
  return { labs: list, active };
}
