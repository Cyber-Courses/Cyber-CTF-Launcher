import type { SelfTestEvent } from "@/lib/tauri";
import { formatElapsed } from "@/lib/format";

// A server self-test's steps as its events move them on, timed by when each event came.

/** A step's last state and detail; how many bytes it moves, once known; when it started running
 *  and when it ended (ms since epoch), as the events came. */
type StepState = { state: string; detail?: string; size?: number; since?: number; end?: number };

/** Each step's state, by step name (none for a step not reached). */
export type StepStates = Partial<Record<string, StepState>>;

/** An event, with when it came (ms since epoch). */
export type Stamped = { e: SelfTestEvent; at: number };

/** `prev`, the state of `e`'s step, moved on by `e`, which came `at`: running from its first
 *  event, ended by its first that isn't running, its size kept once known. */
export function stepAfter(prev: StepState | undefined, { e, at }: Stamped): StepState {
  return {
    state: e.state,
    detail: e.detail ?? undefined,
    size: e.size ?? prev?.size,
    since: prev?.since ?? at,
    end: e.state === "running" ? undefined : (prev?.end ?? at),
  };
}

/** Each step's state, from the events in the order they came. */
export function statesOf(events: Stamped[]): StepStates {
  const states: StepStates = {};
  for (const s of events) states[s.e.step] = stepAfter(states[s.e.step], s);
  return states;
}

/** How long step `s` has run (still ticking at `now`), or took; none before it started or when
 *  skipped. */
export function stepTime(s: StepState | undefined, now: number): string | null {
  if (s?.since === undefined || s.state === "skip") return null;
  return formatElapsed((s.end ?? now) - s.since);
}
