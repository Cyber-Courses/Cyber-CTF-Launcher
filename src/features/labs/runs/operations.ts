"use client";

import { activeOperations, deployingLabs, type ActiveOperation } from "@/lib/tauri";
import { sharedPoll, useSharedPoll } from "@/lib/shared-poll";
import { translate } from "@/lib/i18n";
import { structurallyEqual } from "@/lib/equal";

// The operations the backend runs on labs right now, polled from the long-lived Rust process,
// and how they read on screen.

/** What a lab shows while an operation runs on it: the containers report "running" well before
 *  a launch or resume is done, and until the end of a shut down or stop. */
export const OPERATION_STATUS: Partial<Record<string, string>> = {
  // Getters: read in the current language each time.
  get launch() {
    return translate("common.operation.status.launch");
  },
  get resume() {
    return translate("common.operation.status.resume");
  },
  get pause() {
    return translate("common.operation.status.pause");
  },
  get shutdown() {
    return translate("common.operation.status.shutdown");
  },
  get stop() {
    return translate("common.operation.status.stop");
  },
};

/**
 * The lab ids the backend is starting or stopping right now, polled from the long-lived Rust
 * process. The in-memory store above is lost when the window reloads (a dev rebuild, a crash, or
 * fast-refresh), but the deploy keeps running; reading this lets a screen show "this lab is
 * starting" again instead of a bare Start button (which would invite a colliding second start).
 */
export function useDeployingLabs(enabled = true): Set<string> {
  return useSharedPoll(deployingPoll, enabled);
}

// One poll of each for the whole window, whichever screens read it (see shared-poll).
const deployingPoll = sharedPoll({ read: () => deployingLabs().then((l) => new Set(l)), ms: 4000, initial: new Set<string>(), onFocus: true });

/** The labs deploying in the backend as last read (empty while nothing reads them). */
export const deployingLabsNow = () => deployingPoll.get();

/** The sidebar's label for an operation, e.g. "Pausing…"; the machine when it targets one. */
export function operationLabel(o: ActiveOperation): string {
  switch (o.op) {
    case "launch":
      // The same word as everywhere else a lab starts (header, list, deployment): "Starting…".
      return `${translate("common.operation.status.launch")}…`;
    case "resume":
      return translate("common.operation.label.resume");
    case "pause":
      return translate("common.operation.label.pause");
    case "shutdown":
      return translate("common.operation.label.shutdown");
    case "provision":
      return o.machine
        ? translate("common.operation.label.provisionMachine", { machine: o.machine.replace(/^isoloom-/, "") })
        : translate("common.operation.label.provision");
    case "attack_vm":
      return translate("common.operation.label.attackVm");
    case "stop":
      return translate("common.operation.label.stop");
  }
}

/**
 * Every operation in flight, by lab, with the step its log is at. Polled often (steps move
 * every few seconds during a VM start), so the sidebar reads like a live progress line.
 */
export function useActiveOperations(enabled = true): Map<string, ActiveOperation> {
  return useSharedPoll(operationsPoll, enabled);
}

// When this window first saw each operation in flight ("lab:op" → ms), kept for the session (a
// reload keeps it): the backend doesn't say when an operation began, and a page opened on a
// running deploy must not count from its own opening. For an operation already running when the
// app started, that is when the app first read it.
const SEEN_KEY = "cyberctf.operations.seen";
let seen: Record<string, number> | null = null;
const seenKey = (labId: string, op: string) => `${labId}:${op}`;

function readSeen(): Record<string, number> {
  if (seen) return seen;
  try {
    seen = JSON.parse(sessionStorage.getItem(SEEN_KEY) ?? "{}") as Record<string, number>;
  } catch {
    seen = {};
  }
  return seen;
}

/** Notes the operations in flight now: a new one is first seen now, one that ended is forgotten. */
export function noteOperations(ops: ActiveOperation[], now = Date.now()) {
  const before = readSeen();
  const next: Record<string, number> = {};
  for (const o of ops) next[seenKey(o.labId, o.op)] = before[seenKey(o.labId, o.op)] ?? now;
  if (structurallyEqual(before, next)) return;
  seen = next;
  try {
    sessionStorage.setItem(SEEN_KEY, JSON.stringify(next));
  } catch {
    /* no storage: kept in memory */
  }
}

/** When this window first saw `op` in flight on `labId` (see above), if it did. */
export const operationSeenAt = (labId: string, op: string): number | undefined => readSeen()[seenKey(labId, op)];

const operationsPoll = sharedPoll({
  read: () =>
    activeOperations().then((l) => {
      noteOperations(l);
      return new Map(l.map((o) => [o.labId, o]));
    }),
  ms: 2000,
  initial: new Map<string, ActiveOperation>(),
  // Read on in a hidden window, slower while nothing runs: the shell starts a lab's attack box
  // when its launch ends (from the website too), which must not wait for the window to show.
  hiddenMs: (ops) => (ops.size > 0 ? 2000 : 10_000),
});

/** The operations in flight as last read (empty while nothing reads them). */
export const activeOperationsNow = () => operationsPoll.get();
