"use client";

import { useEffect } from "react";
import { labsWatch } from "@/lib/tauri";
import { connectRunEvents, onLabEvent } from "@/lib/tauri/run-events";
import { takeLabEvent, useLabState } from "@/features/labs/lab-store";
import { ignore } from "@/lib/failure";
import { registerReset } from "@/lib/module-state";
import type { Lab } from "@/features/labs/use-labs";

// The labs' statuses, kept current by the core's reconcile, not by a poll: this window tells the
// core which labs it shows (each with its catalogue runtime) and which lab pages are open; the
// core reads what may have changed (what its scan of this machine sees up, what an operation acts
// on, what ran, the open pages, and every lab now and then) and tells what did, as run events
// (`onLabEvent`) the lab store takes in.

// Every screen's catalogue (they can differ for a moment around a sign-in change), and the lab
// pages open, counted.
const catalogues = new Map<symbol, Lab[]>();
const open = new Map<string, number>();
// What was last told to the core, so the same set isn't told again.
let told = "";
let unfeed: (() => void) | null = null;

registerReset(() => {
  catalogues.clear();
  open.clear();
  told = "";
  unfeed?.();
  unfeed = null;
});

/** Takes the core's lab events into the lab store, once per window. */
function feed() {
  if (unfeed) return;
  unfeed = onLabEvent(takeLabEvent);
  connectRunEvents().catch(ignore("the labs' statuses arrive once the core answers"));
}

/** Tells the core what this window watches, when that changed. */
function tell() {
  const byId = new Map<string, Lab>();
  for (const labs of catalogues.values()) for (const l of labs) byId.set(l.id, l);
  const labs = [...byId.values()].flatMap((l) => (l.runtime ? [{ id: l.id, runtime: l.runtime.runtime }] : []));
  const pages = [...open.keys()];
  const key = JSON.stringify([labs, pages]);
  if (key === told) return;
  told = key;
  labsWatch(labs, pages).catch(ignore("watched again with the next change"));
}

/** Keeps the statuses of `labs` current while a screen shows them (see the module). */
export function useWatchLabs(labs: Lab[] | null) {
  useEffect(() => {
    if (!labs) return;
    const key = Symbol("catalogue");
    catalogues.set(key, labs);
    feed();
    tell();
    return () => {
      catalogues.delete(key);
      // After the effects that follow (a new catalogue in its place): one watch, not an empty one
      // between.
      queueMicrotask(tell);
    };
  }, [labs]);
}

/** The labs the core's scan last saw up on this machine (the sidebar's running entries), kept
 *  current by its events while `enabled`. */
export function useScannedRunning(enabled: boolean): Set<string> {
  useEffect(() => {
    if (enabled) feed();
  }, [enabled]);
  return useLabState("scanRunning");
}

/** The lab page open on `labId`: the core reads it at every pass while it is. */
export function useWatchLab(labId: string) {
  useEffect(() => {
    open.set(labId, (open.get(labId) ?? 0) + 1);
    tell();
    return () => {
      const n = (open.get(labId) ?? 1) - 1;
      if (n > 0) open.set(labId, n);
      else open.delete(labId);
      queueMicrotask(tell);
    };
  }, [labId]);
}
