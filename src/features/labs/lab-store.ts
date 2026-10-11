"use client";

import { useSyncExternalStore } from "react";
import { labStatus, type LabStatus } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";

/**
 * What this machine knows about the labs, kept outside the React tree and shared by every
 * screen (as deploy-store is for runs): each lab's last status, which labs were asked at least
 * once, what the infrastructure scan sees running, and the catalogue last loaded. A lab page
 * opened again renders at once from it instead of waiting on a fresh probe (seconds on a busy
 * machine), and two screens mounted at once read and refresh the same state.
 */
type LabState = {
  statuses: Record<string, LabStatus>;
  /** Labs whose status has been asked at least once (answered or not). */
  probed: Set<string>;
  /** Labs the infrastructure scan (docker/vagrant) reports running. */
  scanRunning: Set<string>;
  /** The catalogue last loaded, for the sign-in state (`key`) it was loaded under. */
  catalogue: { key: unknown; labs: Lab[] } | null;
};

let state: LabState = { statuses: {}, probed: new Set(), scanRunning: new Set(), catalogue: null };
const listeners = new Set<() => void>();

function set(patch: Partial<LabState>) {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}

function subscribe(cb: () => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** One field of the lab state, re-rendering when it changes. */
export function useLabState<K extends keyof LabState>(key: K): LabState[K] {
  return useSyncExternalStore(
    subscribe,
    () => state[key],
    () => state[key],
  );
}

/** The state now, outside React (tests, event handlers). */
export function getLabState(): LabState {
  return state;
}

export function catalogueFor(key: unknown): Lab[] | null {
  return state.catalogue && state.catalogue.key === key ? state.catalogue.labs : null;
}

export function setCatalogue(key: unknown, labs: Lab[]) {
  set({ catalogue: { key, labs } });
}

// One status read per lab at a time: a slow read (a busy VirtualBox can take seconds) must not
// let the poll interval stack a second, third, … read on top of it; a poll joins the read in
// flight. A `fresh` read (right after an operation) always starts, and only the newest read of
// a lab may set its status: a poll that began before a shutdown ended must not land after it
// with the old "running" and bring back the running lab's buttons.
const inFlight = new Map<string, Promise<void>>();
const latest = new Map<string, number>();
// When each lab's latest fresh read began: a scan begun before it is older news for that lab.
const freshAt = new Map<string, number>();
// When this app last stopped, paused or shut down each lab. The backend answers a scan asked
// while another is running with that scan's last result, which can predate the stop: until a
// status read begun after the stop sees the lab running again, the scan's word is not taken for it.
const stoppedAt = new Map<string, number>();

/** This app just took `labId` down (stop, pause, shut down): see `stoppedAt`. */
export function noteStopped(labId: string) {
  stoppedAt.set(labId, Date.now());
  if (state.scanRunning.has(labId)) set({ scanRunning: new Set([...state.scanRunning].filter((id) => id !== labId)) });
}

/** Reads a lab's status into the store; see above for `fresh`. */
export function refreshStatus(lab: Lab, { fresh = false }: { fresh?: boolean } = {}): Promise<void> {
  if (!lab.runtime) return Promise.resolve();
  const pending = inFlight.get(lab.id);
  if (pending && !fresh) return pending;
  const mine = (latest.get(lab.id) ?? 0) + 1;
  latest.set(lab.id, mine);
  const began = Date.now();
  if (fresh) freshAt.set(lab.id, began);
  const read: Promise<void> = labStatus(lab.id, lab.runtime.runtime)
    .then((s) => {
      if (latest.get(lab.id) !== mine) return;
      // Seen running again since this app took it down (started from the website, say): the
      // scan counts for it again.
      if (s.running && (stoppedAt.get(lab.id) ?? Infinity) <= began) stoppedAt.delete(lab.id);
      set({ statuses: { ...state.statuses, [lab.id]: s } });
      // Down now: the last scan's "running" for it is out of date.
      if (fresh && !s.running && state.scanRunning.has(lab.id)) {
        set({ scanRunning: new Set([...state.scanRunning].filter((id) => id !== lab.id)) });
      }
    })
    .catch(() => {
      /* not installed / not running - leave status unknown */
    })
    .finally(() => {
      if (inFlight.get(lab.id) === read) inFlight.delete(lab.id);
      if (!state.probed.has(lab.id)) set({ probed: new Set(state.probed).add(lab.id) });
    });
  inFlight.set(lab.id, read);
  return read;
}

/** What a scan begun at `began` found running, but the labs read fresh since it began; returns
 *  them, to read their status. Labs this app took down since are left out of the running set
 *  (see `stoppedAt`) but still returned, so a status read can confirm them. */
export function setScanRunning(ids: string[], began: number): string[] {
  const kept = ids.filter((id) => (freshAt.get(id) ?? 0) < began);
  set({ scanRunning: new Set(kept.filter((id) => !stoppedAt.has(id))) });
  return kept;
}
