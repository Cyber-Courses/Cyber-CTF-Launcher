"use client";

import { useEffect } from "react";
import { runningLabs, type LabStatus } from "@/lib/tauri";
import { activeOperationsNow, deployingLabsNow } from "@/features/labs/runs/operations";
import { getDeploySnapshot } from "@/features/labs/runs/deploy-runs";
import { getLabState, refreshStatus, setScanRunning } from "@/features/labs/lab-store";
import { sharedPoll } from "@/lib/shared-poll";
import { onVisibilityChange } from "@/lib/visibility";
import type { Lab } from "@/features/labs/use-labs";

/** How often the running scan runs, and the labs that can change on their own are read. */
export const LAB_POLL_MS = 6000;

/**
 * The labs whose status is read on every poll: those that can change without this window doing
 * anything. Running ones (the scan sees them, or their last status says so: a lab on a server or
 * in the cloud, which the scan can't see, can stop or expire), busy ones (an operation in flight,
 * here or from the website) and the lab whose page is open. A lab that is none of these changes
 * only through this app, which reads it after each operation, or shows up in the scan (started
 * from the website), which reads it then.
 */
export function hotLabs<L extends { id: string }>(
  labs: L[],
  { scanRunning, statuses, busy, watched }: { scanRunning: Set<string>; statuses: Partial<Record<string, LabStatus>>; busy: Set<string>; watched: Set<string> },
): L[] {
  return labs.filter((l) => scanRunning.has(l.id) || busy.has(l.id) || watched.has(l.id) || (statuses[l.id]?.running ?? false));
}

// Every screen's catalogue (they can differ for a moment around a sign-in change), and the lab
// pages open, counted.
const catalogues = new Map<symbol, Lab[]>();
const watched = new Map<string, number>();
// The labs read on the last poll, so one that drops out (it stopped, its operation ended, its
// page closed) is read once more and settles on its final status.
let lastHot = new Set<string>();
// When every lab was last read (on focus), so focusing back and forth doesn't read them all each time.
let lastAll = 0;

function allLabs(): Lab[] {
  const byId = new Map<string, Lab>();
  for (const labs of catalogues.values()) for (const l of labs) byId.set(l.id, l);
  return [...byId.values()];
}

function busyIds(): Set<string> {
  const busy = new Set<string>([...activeOperationsNow().keys(), ...deployingLabsNow()]);
  for (const [id, run] of Object.entries(getDeploySnapshot().runs)) if (run?.busy) busy.add(id);
  return busy;
}

/** One poll: the hot labs' statuses, and the running scan, then the labs the scan saw change. */
function poll(): Promise<string[]> {
  const labs = allLabs();
  const { scanRunning: before, statuses } = getLabState();
  const hot = new Set(hotLabs(labs, { scanRunning: before, statuses, busy: busyIds(), watched: new Set(watched.keys()) }).map((l) => l.id));
  const read = new Set([...hot, ...lastHot]);
  lastHot = hot;
  labs.filter((l) => read.has(l.id)).forEach((l) => void refreshStatus(l));
  const began = Date.now();
  return runningLabs().then((all) => {
    // A lab read fresh after this scan began (an operation just ended on it) is left out.
    const kept = setScanRunning(all, began);
    const after = getLabState().scanRunning;
    // What the scan sees (a lab started from the website, one this app took down that runs
    // again) and what it no longer sees, unless just read above.
    const changed = new Set([...kept, ...[...before].filter((id) => !after.has(id))].filter((id) => !read.has(id)));
    allLabs()
      .filter((l) => changed.has(l.id))
      .forEach((l) => void refreshStatus(l));
    return all;
  });
}

const labPoll = sharedPoll<string[]>({ read: poll, ms: LAB_POLL_MS, initial: [] });

/** Reads every lab once (the window came back), at most once per poll interval. */
function readAll() {
  if (Date.now() - lastAll < LAB_POLL_MS) return;
  lastAll = Date.now();
  allLabs().forEach((l) => void refreshStatus(l));
}

/**
 * Keeps the labs' statuses current while a screen shows them: one poll for the whole window
 * (see `hotLabs` for which labs it reads), paused while the window is hidden. Every lab is read
 * again when the window regains focus or shows again.
 */
export function useLabPolling(labs: Lab[] | null) {
  useEffect(() => {
    if (!labs) return;
    const key = Symbol("catalogue");
    catalogues.set(key, labs);
    return () => {
      catalogues.delete(key);
    };
  }, [labs]);
  useEffect(() => {
    const off = labPoll.subscribe(() => {});
    window.addEventListener("focus", readAll);
    const offVisibility = onVisibilityChange((visible) => {
      if (visible) readAll();
    });
    return () => {
      off();
      window.removeEventListener("focus", readAll);
      offVisibility();
    };
  }, []);
}

/** The lab page open on `labId`: its status is read on every poll while it is. */
export function useWatchLab(labId: string) {
  useEffect(() => {
    watched.set(labId, (watched.get(labId) ?? 0) + 1);
    return () => {
      const n = (watched.get(labId) ?? 1) - 1;
      if (n > 0) watched.set(labId, n);
      else watched.delete(labId);
    };
  }, [labId]);
}
