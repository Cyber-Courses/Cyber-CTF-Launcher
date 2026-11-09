"use client";

import { useSyncExternalStore } from "react";
import { labStatus, type LabStatus } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";
import { structurallyEqual } from "@/lib/equal";
import { describeError } from "@/lib/errors";
import { warn } from "@/lib/failure";

/**
 * What this machine knows about the labs, kept outside the React tree and shared by every
 * screen (as runs/deploy-runs is for runs): each lab's last status, why its last read failed,
 * which labs were asked at least once, what the infrastructure scan sees running, and the catalogue last loaded. A lab page
 * opened again renders at once from it instead of waiting on a fresh probe (seconds on a busy
 * machine), and two screens mounted at once read and refresh the same state.
 */
type LabState = {
  statuses: Record<string, LabStatus>;
  /** Why a lab's last status read failed (Docker down, an unreadable run record, files that can't
   *  be generated again), until a read succeeds. Its last status, if any, is kept meanwhile. */
  statusErrors: Record<string, unknown>;
  /** Labs whose status has been asked at least once (answered or not). */
  probed: Set<string>;
  /** Labs the infrastructure scan (Docker, the hypervisors) sees up: only which labs to read
   *  again (`hotLabs`), never whether one runs, which is its status's word alone. */
  scanRunning: Set<string>;
  /** The catalogue last loaded, for the sign-in state (`key`) it was loaded under. */
  catalogue: { key: unknown; labs: Lab[] } | null;
};

let state: LabState = { statuses: {}, statusErrors: {}, probed: new Set(), scanRunning: new Set(), catalogue: null };
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

/** Reads a lab's status into the store; see above for `fresh`. */
export function refreshStatus(lab: Lab, { fresh = false }: { fresh?: boolean } = {}): Promise<void> {
  if (!lab.runtime) return Promise.resolve();
  const pending = inFlight.get(lab.id);
  if (pending && !fresh) return pending;
  const mine = (latest.get(lab.id) ?? 0) + 1;
  latest.set(lab.id, mine);
  const read: Promise<void> = labStatus(lab.id, lab.runtime.runtime)
    .then((s) => {
      if (latest.get(lab.id) !== mine) return;
      setStatusError(lab.id, undefined);
      // The same answer again (most polls) keeps the current object: nothing re-renders.
      if (!structurallyEqual(state.statuses[lab.id], s)) set({ statuses: { ...state.statuses, [lab.id]: s } });
    })
    .catch((e: unknown) => {
      // A lab that isn't installed or isn't running answers (not running); a failed read is
      // something wrong with the lab or this machine, said on the lab's page.
      if (latest.get(lab.id) !== mine) return;
      setStatusError(lab.id, e);
    })
    .finally(() => {
      if (inFlight.get(lab.id) === read) inFlight.delete(lab.id);
      if (!state.probed.has(lab.id)) set({ probed: new Set(state.probed).add(lab.id) });
    });
  inFlight.set(lab.id, read);
  return read;
}

/** Records why `id`'s status read failed (`undefined`: it succeeded), logging a new failure once,
 *  not on every poll that repeats it. */
function setStatusError(id: string, error: unknown) {
  const had = id in state.statusErrors;
  if (error === undefined ? !had : had && describeError(state.statusErrors[id]) === describeError(error)) return;
  if (error !== undefined) warn(`reading the status of lab ${id}`)(error);
  const others = Object.entries(state.statusErrors).filter(([lab]) => lab !== id);
  set({ statusErrors: Object.fromEntries(error === undefined ? others : [...others, [id, error]]) });
}

/** What the last scan saw up. */
export function setScanRunning(ids: string[]) {
  const running = new Set(ids);
  if (!structurallyEqual(state.scanRunning, running)) set({ scanRunning: running });
}
