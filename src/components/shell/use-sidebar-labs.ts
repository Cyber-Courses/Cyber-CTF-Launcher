"use client";

import { useEffect, useMemo, useState } from "react";
import { apiQuery, type ActiveOperation } from "@/lib/tauri";
import { useDeployingLabs } from "@/features/labs/runs/operations";
import { useLabState } from "@/features/labs/lab-store";
import { useImportedLabs } from "@/features/labs/imported-labs";
import { useScannedRunning } from "@/features/labs/lab-polling";
import { activeLabs, type SidebarLab } from "@/components/shell/shell-model";
import { warn } from "@/lib/failure";

/**
 * The lab list behind the sidebar and the command palette (a light one, to jump straight to a
 * lab), loaded again on sign-in changes, with the labs imported from a folder after it, and the
 * labs running or starting right now, shown as their own sidebar entries so one is a click away
 * wherever you are. Deploys come from the backend's operations (so they survive a reload); the
 * running set is the window's running scan (`useScannedRunning`).
 */
export function useSidebarLabs(loggedIn: boolean | undefined, ops: Map<string, ActiveOperation>, enabled = true) {
  const [labs, setLabs] = useState<SidebarLab[]>([]);
  useEffect(() => {
    // Not in a window that shows none of it (Settings).
    if (!enabled) return;
    // A sign-in change starts a new read: the one in flight from before it must not land last.
    let alive = true;
    apiQuery<{ labs: SidebarLab[] }>("sidebarLabs")
      .then((d) => {
        if (alive) setLabs(d.labs);
      })
      .catch(
        warn("reading the sidebar's labs", () => {
          if (alive) setLabs([]);
        }),
      );
    return () => {
      alive = false;
    };
  }, [loggedIn, enabled]);

  const deploying = useDeployingLabs(enabled);
  // Scanned again as soon as an operation starts or ends (the deploying labs are among them): a
  // lab just shut down otherwise kept its "Running" line until the next poll.
  const busyKey = [...ops.keys()].sort().join(",");
  const runningIds = useScannedRunning(enabled, busyKey);
  const statuses = useLabState("statuses");
  // When this light list couldn't be read (offline), the catalogue a screen loaded since (its
  // Retry) stands in, so the entries and the palette come back with it.
  const catalogue = useLabState("catalogue");
  const catalogued = labs.length > 0 ? labs : (catalogue?.labs ?? labs);
  // The labs imported from a folder, which no catalogue lists: by their own title, to their page.
  const imported = useImportedLabs(enabled);
  const list = useMemo(
    () => [...catalogued, ...(imported ?? []).map(({ id, slug, title, category }): SidebarLab => ({ id, slug, title, category }))],
    [catalogued, imported],
  );

  // The same list object until one of its inputs changes, so the sidebar's entries don't re-render.
  const active = useMemo(() => activeLabs(list, deploying, ops, runningIds, statuses), [list, deploying, ops, runningIds, statuses]);
  return { labs: list, active };
}
