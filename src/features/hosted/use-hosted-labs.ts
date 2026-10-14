"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { apiQuery } from "@/lib/tauri";
import { ignore, warn } from "@/lib/failure";
import { structurallyEqual } from "@/lib/equal";
import { usePoll } from "@/lib/use-poll";
import { translate } from "@/lib/i18n";
import { describeError } from "@/lib/errors";
import { useLatest } from "@/lib/use-latest";

export interface HostedEndpoint {
  port: number;
  url: string;
}

/** Mirrors the backend LabSessionState enum. */
export type HostedState = "REQUESTED" | "CLAIMED" | "PULLING" | "RUNNING" | "FAILED" | "STOPPED" | "EXPIRED";

export interface HostedSession {
  id: string;
  /** The lab this session runs, so a view can tell whether it's "its" session. */
  labId: string;
  /** "hosted" for a hosted lab; an agent target id, or null, otherwise. */
  target: string | null;
  state: HostedState;
  endpoints: HostedEndpoint[];
  message: string | null;
  expiresAt: string;
}

const SETTLED: HostedState[] = ["RUNNING", "FAILED", "STOPPED", "EXPIRED"];

/**
 * The player's active hosted session: hydrated on mount from `myActiveLabSession` (so it
 * persists across views and restarts), launched with `launch`, polled until it settles, and
 * cleared with `stop`. Only hosted sessions are tracked here; other targets run through the
 * local runtime. Used by the lab page.
 */
export function useHostedLabs() {
  const [session, setSession] = useState<HostedSession | null>(null);
  const [busyLab, setBusyLab] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Stop polling a session that never settles, so a stuck backend launch doesn't spin forever.
  const pollDeadline = useRef<{ id: string; until: number } | null>(null);
  // The session that gave up so (its polling stops).
  const [gaveUp, setGaveUp] = useState<string | null>(null);

  // The session as the player last set it: a Launch or Stop begins a new one, so the hydration
  // (or an earlier launch) answering after it can't bring back a session just stopped.
  const begin = useLatest();

  // Hydrate from the backend once (requires login; a logged-out / errored call just leaves it null).
  useEffect(() => {
    const isLatest = begin();
    apiQuery<{ myActiveLabSession: HostedSession | null }>("activeSession")
      .then((d) => {
        const s = d.myActiveLabSession;
        if (isLatest() && s && s.target === "hosted" && !SETTLED.slice(1).includes(s.state)) setSession(s);
      })
      .catch(warn("reading the active hosted session"));
  }, [begin]);

  const launch = useCallback(
    async (labId: string) => {
      const isLatest = begin();
      setBusyLab(labId);
      setError(null);
      try {
        const d = await apiQuery<{ requestLabLaunch: HostedSession }>("launchHosted", { labId });
        if (isLatest()) setSession(d.requestLabLaunch);
        // Stopped while it was being requested: the session it made must not run on unseen.
        else void apiQuery("stopSession", { id: d.requestLabLaunch.id }).catch(warn("stopping a hosted session stopped while it was requested"));
      } catch (e) {
        if (isLatest()) setError(describeError(e));
      } finally {
        // The request returns the REQUESTED session fast; the session state drives the rest.
        if (isLatest()) setBusyLab(null);
      }
    },
    [begin],
  );

  // Poll the active session until it settles (RUNNING with endpoints, or FAILED/STOPPED/EXPIRED),
  // every 2.5 s, paused while the window is hidden. A failed or empty poll just polls again. A
  // poll in flight when the session changes (e.g. stop() clears it) is dropped: it must not
  // resurrect a session the user just stopped.
  const pending = session && !SETTLED.includes(session.state) ? session.id : null;
  // Give a launch a bounded time to come up; a session stuck in REQUESTED/CLAIMED/PULLING past
  // it stops polling with a message instead of spinning forever.
  const checkDeadline = (id: string) => {
    if (!pollDeadline.current || pollDeadline.current.id !== id) pollDeadline.current = { id, until: Date.now() + 10 * 60 * 1000 };
    if (Date.now() <= pollDeadline.current.until) return;
    setError(translate("hosted.slowStart"));
    setGaveUp(id);
  };
  usePoll(() => apiQuery<{ labSession: HostedSession | null }>("labSession", { id: pending }), 2500, {
    enabled: pending !== null && gaveUp !== pending,
    restartKey: pending,
    leading: false,
    onValue: (d) => {
      const next = d.labSession;
      if (next) setSession((prev) => (structurallyEqual(prev, next) ? prev : next));
      if (pending) checkDeadline(pending);
    },
    onError: (e) => {
      ignore("polled again in a moment")(e);
      if (pending) checkDeadline(pending);
    },
  });

  const stop = useCallback(async () => {
    begin();
    const current = session;
    setSession(null);
    setBusyLab(null);
    // The session is already off the page; a stop that fails says so, or it would keep running unseen.
    if (current) await apiQuery("stopSession", { id: current.id }).catch((e) => setError(translate("hosted.stopFailed", { error: describeError(e) })));
  }, [session, begin]);

  return { session, busyLab, error, launch, stop };
}
