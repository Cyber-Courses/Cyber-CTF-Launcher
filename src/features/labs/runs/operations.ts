"use client";

import { useEffect, useMemo, useSyncExternalStore } from "react";
import type { ActiveOperation } from "@/lib/tauri";
import { translate, type MessageKey } from "@/lib/i18n";
import { ignore } from "@/lib/failure";
import { machineLabel } from "@/lib/format";
import { DONE_MARK, FAILED_MARK } from "@/lib/tauri/generated/DeployLog";
import { connectRunEvents, inFlight, runEventsNow, subscribeRunEvents, type OperationRun, type RunEventsState } from "@/lib/tauri/run-events";

// The operations the core runs on labs right now, read from the run events (`run-events`: no
// poll), and how they read on screen.

/** A lab operation, as the backend names it. */
export type LabOperation = ActiveOperation["op"];

/** The word a lab shows while `op` runs on it ("Starting", "Pausing", "Provisioning"), the same in
 *  the list, the lab page, the sidebar and Home: the containers report "running" well before a
 *  launch or resume is done, and until the end of a shut down or stop. */
export const operationStatus = (op: LabOperation, t: (key: MessageKey) => string = translate): string => t(`common.operation.status.${op}`);

/** Whether a lab keeps its running phase while `op` runs on it: a provisioning run works on a lab
 *  that is up and usable, so the list and the page still say "Running"; every other operation
 *  names the lab's state while it runs. */
export const keepsRunningPhase = (op: LabOperation): boolean => op === "provision";

/**
 * The labs the backend deploys among `ops`, the operations in flight, as each says
 * (`deploying`).
 */
export function deployingOf(ops: ReadonlyMap<string, ActiveOperation>): Set<string> {
  return new Set([...ops.values()].filter((o) => o.deploying).map((o) => o.labId));
}

/** Longest step shown in the sidebar, in characters (Rust's `progress::STEP_MAX`). */
const STEP_MAX = 72;

/** The step an operation's log is at: the last Vagrant action line (`==> dc01: Booting VM...`) as
 *  `dc01: Booting VM...`, else the last non-empty line, kept short; null for none. */
function lastStep(lines: readonly string[]): string | null {
  // From the end: the last action line wins, else the last other line; a long log is read only
  // back to its last action.
  let last: string | null = null;
  let action: string | null = null;
  for (let i = lines.length - 1; i >= 0 && action === null; i--) {
    const l = lines[i].trim();
    if (!l || l.startsWith(DONE_MARK) || l.startsWith(FAILED_MARK)) continue;
    if (l.startsWith("==>")) {
      const rest = l.slice(3).trim();
      const colon = rest.indexOf(":");
      if (colon >= 0 && rest.slice(colon + 1).trim()) action = rest;
      continue;
    }
    last ??= l;
  }
  const step = action ?? last;
  if (step === null) return null;
  return step.length > STEP_MAX ? `${step.slice(0, STEP_MAX - 1)}…` : step;
}

// Each run's reading, kept while the run is the same object: a change of one operation doesn't
// read every other one's log again.
const read = new WeakMap<OperationRun, ActiveOperation>();

/** An operation in flight as the screens read it: what it is, the step it is at. */
function asActive(r: OperationRun): ActiveOperation {
  let active = read.get(r);
  if (!active) {
    active = { labId: r.labId, op: r.op, machine: r.machine, deploying: r.deploying, step: lastStep(r.lines), runtime: r.runtime };
    read.set(r, active);
  }
  return active;
}

/** Every operation in flight in `s`, by lab (a lab's latest one). */
function activeOf(s: RunEventsState): Map<string, ActiveOperation> {
  return new Map(inFlight(s).map((r) => [r.labId, asActive(r)]));
}

// The map handed out, the same object until the operations change (readers compare by identity).
let lastState: RunEventsState | null = null;
let lastActive = new Map<string, ActiveOperation>();
function activeNow(): Map<string, ActiveOperation> {
  const s = runEventsNow();
  if (s !== lastState) {
    lastState = s;
    lastActive = activeOf(s);
  }
  return lastActive;
}

/**
 * Every operation in flight, by lab, with the step its log is at, kept current by the run events
 * while `enabled` (the window subscribes to them on first use).
 */
export function useActiveOperations(enabled = true): Map<string, ActiveOperation> {
  useEffect(() => {
    if (enabled) connectRunEvents().catch(ignore("the operations show once the core answers"));
  }, [enabled]);
  const ops = useSyncExternalStore(subscribeRunEvents, activeNow, activeNow);
  return enabled ? ops : EMPTY;
}

const EMPTY = new Map<string, ActiveOperation>();

/**
 * The lab ids the backend is deploying right now (`deployingOf`): a window that reloaded while a
 * deploy runs shows "this lab is starting" again instead of a bare Start button (which would
 * invite a colliding second start). The same set until the operations change.
 */
export function useDeployingLabs(enabled = true): Set<string> {
  const ops = useActiveOperations(enabled);
  return useMemo(() => deployingOf(ops), [ops]);
}

/** Whether any lab operation is in flight, of any kind (a stop and a pause too). */
export function useLabsBusy(): boolean {
  return useActiveOperations().size > 0;
}

/** The sidebar's label for an operation, e.g. "Pausing…": the word the operation has everywhere
 *  else (header, list, deployment), running; the machine when it targets one. */
export function operationLabel(o: ActiveOperation): string {
  if (o.op === "provision" && o.machine) return translate("common.operation.label.provisionMachine", { machine: machineLabel(o.machine) });
  return `${operationStatus(o.op)}…`;
}

/** When `op`, in flight on `labId`, began (ms), as the core says: a page opened on a running
 *  deploy counts from there, not from its own opening. */
export function operationSeenAt(labId: string, op: string): number | undefined {
  return inFlight(runEventsNow()).find((r) => r.labId === labId && r.op === op)?.startedAt;
}
