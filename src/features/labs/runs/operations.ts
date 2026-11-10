"use client";

import { activeOperations, type ActiveOperation } from "@/lib/tauri";
import { useMemo } from "react";
import { sharedPoll, useSharedPoll } from "@/lib/shared-poll";
import { translate } from "@/lib/i18n";
import { structurallyEqual } from "@/lib/equal";
import { ignore } from "@/lib/failure";
import { readSession, writeSession } from "@/lib/local-store";
import { machineLabel } from "@/lib/format";

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
 * The labs the backend deploys among `ops`, the operations in flight, as each says (Rust's
 * `ActiveOperation::deploying`: what `deploying_labs` answers), read from the one poll of the
 * operations rather than a second one.
 */
export function deployingOf(ops: ReadonlyMap<string, ActiveOperation>): Set<string> {
  return new Set([...ops.values()].filter((o) => o.deploying).map((o) => o.labId));
}

/**
 * The lab ids the backend is deploying right now (`deployingOf`), from the operations polled
 * from the long-lived Rust process. The in-memory store above is lost when the window reloads (a
 * dev rebuild, a crash, or fast-refresh), but the deploy keeps running; reading this lets a
 * screen show "this lab is starting" again instead of a bare Start button (which would invite a
 * colliding second start). The same set until the operations change.
 */
export function useDeployingLabs(enabled = true): Set<string> {
  const ops = useActiveOperations(enabled);
  return useMemo(() => deployingOf(ops), [ops]);
}

/** The labs deploying in the backend as last read (empty while nothing reads them). */
export const deployingLabsNow = () => deployingOf(operationsPoll.get());

/** The sidebar's label for an operation, e.g. "Pausing…": the word the operation has everywhere
 *  else (header, list, deployment), running; the machine when it targets one. */
export function operationLabel(o: ActiveOperation): string {
  if (o.op === "provision" && o.machine) return translate("common.operation.label.provisionMachine", { machine: machineLabel(o.machine) });
  return `${translate(`common.operation.status.${o.op}`)}…`;
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
  seen ??= parseSeen(readSession(SEEN_KEY));
  return seen;
}

/** The kept record; none when there is none, or one this window can't read. */
function parseSeen(raw: string | null): Record<string, number> {
  try {
    return raw ? (JSON.parse(raw) as Record<string, number>) : {};
  } catch (e) {
    return ignore("a record of the operations seen that isn't JSON: they count from now", () => ({}))(e);
  }
}

/** Notes the operations in flight now: a new one is first seen now, one that ended is forgotten. */
export function noteOperations(ops: ActiveOperation[], now = Date.now()) {
  const before = readSeen();
  const next: Record<string, number> = {};
  for (const o of ops) next[seenKey(o.labId, o.op)] = before[seenKey(o.labId, o.op)] ?? now;
  if (structurallyEqual(before, next)) return;
  seen = next;
  writeSession(SEEN_KEY, JSON.stringify(next));
}

/** When this window first saw `op` in flight on `labId` (see above), if it did. */
export const operationSeenAt = (labId: string, op: string): number | undefined => readSeen()[seenKey(labId, op)];

// One poll for the whole window, whichever screens read it (see shared-poll).
const operationsPoll = sharedPoll({
  read: () =>
    activeOperations().then((l) => {
      noteOperations(l);
      return new Map(l.map((o) => [o.labId, o]));
    }),
  ms: 2000,
  initial: new Map<string, ActiveOperation>(),
  // And at once when the window comes back: a deploy may have started or ended meanwhile.
  onFocus: true,
  // Read on in a hidden window, slower while nothing runs: the shell starts a lab's attack box
  // when its launch ends (from the website too), which must not wait for the window to show.
  hiddenMs: (ops) => (ops.size > 0 ? 2000 : 10_000),
});

/** The operations in flight as last read (empty while nothing reads them). */
export const activeOperationsNow = () => operationsPoll.get();
