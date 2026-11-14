"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { describeError } from "@/lib/errors";

/** A user action as the page shows it: not started (or done), under way, or failed with why. */
export type ActionState = { state: "idle" } | { state: "running" } | { state: "failed"; error: string };

/** A started action's handle: its state, and `run`, which starts it unless it is under way. */
export type Action<A extends unknown[]> = {
  state: ActionState;
  /** Under way: what disables its button. */
  running: boolean;
  /** Why the last run failed; null once a new run starts. */
  error: string | null;
  /** Starts the action, or does nothing while one is under way (a second click, a second entry
   *  point). The guard is taken at once, before any await, so two clicks in one frame, or a click
   *  while a dialog the action opened is still up, start it once. */
  run: (...args: A) => void;
};

/**
 * One user action that must not run twice at once (save, remove, import, install): its state as
 * one value, and a re-entry guard held from the click to the end, a failure included. A
 * rejection is kept as the state's reason; `act` handles what it must tell itself.
 */
export function useAction<A extends unknown[]>(act: (...args: A) => Promise<unknown>): Action<A> {
  const latest = useRef(act);
  useEffect(() => {
    latest.current = act;
  });
  const under = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const [state, setState] = useState<ActionState>({ state: "idle" });
  const run = useCallback((...args: A) => {
    if (under.current) return;
    under.current = true;
    setState({ state: "running" });
    void latest
      .current(...args)
      .then(
        () => {
          if (alive.current) setState({ state: "idle" });
        },
        (e: unknown) => {
          if (alive.current) setState({ state: "failed", error: describeError(e) });
        },
      )
      .finally(() => {
        under.current = false;
      });
  }, []);
  return { state, running: state.state === "running", error: state.state === "failed" ? state.error : null, run };
}
