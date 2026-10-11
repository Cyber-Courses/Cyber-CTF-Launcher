"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { apiQuery } from "@/lib/tauri";
import { ignore, warn } from "@/lib/failure";
import { translate } from "@/lib/i18n";

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
  // Bumped after a poll that brought no new session (an error, or no answer), so the next poll
  // is still armed: re-arming only on a new session object stopped polling after one failure.
  const [pollTick, setPollTick] = useState(0);

  // Hydrate from the backend once (requires login; a logged-out / errored call just leaves it null).
  useEffect(() => {
    apiQuery<{ myActiveLabSession: HostedSession | null }>("activeSession")
      .then((d) => {
        const s = d.myActiveLabSession;
        if (s && s.target === "hosted" && !SETTLED.slice(1).includes(s.state)) setSession(s);
      })
      .catch(warn("reading the active hosted session"));
  }, []);

  const launch = useCallback(async (labId: string) => {
    setBusyLab(labId);
    setError(null);
    try {
      const d = await apiQuery<{ requestLabLaunch: HostedSession }>("launchHosted", { labId });
      setSession(d.requestLabLaunch);
    } catch (e) {
      setError(String(e));
    } finally {
      // The request returns the REQUESTED session fast; the session state drives the rest.
      setBusyLab(null);
    }
  }, []);

  // Poll the active session until it settles (RUNNING with endpoints, or FAILED/STOPPED/EXPIRED).
  useEffect(() => {
    if (!session || SETTLED.includes(session.state)) {
      pollDeadline.current = null;
      return;
    }
    // Give a launch a bounded time to come up; a session stuck in REQUESTED/CLAIMED/PULLING past
    // it stops polling with a message instead of spinning forever.
    if (!pollDeadline.current || pollDeadline.current.id !== session.id) {
      pollDeadline.current = { id: session.id, until: Date.now() + 10 * 60 * 1000 };
    }
    if (Date.now() > pollDeadline.current.until) {
      setError(translate("hosted.slowStart"));
      return;
    }
    // Cancelled when the session changes (e.g. stop() clears it): a poll already in flight must
    // not resolve afterwards and resurrect a session the user just stopped.
    let cancelled = false;
    const t = setTimeout(() => {
      apiQuery<{ labSession: HostedSession | null }>("labSession", { id: session.id })
        .then((d) => {
          if (cancelled) return;
          if (d.labSession) setSession(d.labSession);
          else setPollTick((n) => n + 1);
        })
        .catch((e) => {
          ignore("polled again in a moment")(e);
          if (!cancelled) setPollTick((n) => n + 1);
        });
    }, 2500);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [session, pollTick]);

  const stop = useCallback(async () => {
    const current = session;
    setSession(null);
    setBusyLab(null);
    // The session is already off the page; a stop that fails says so, or it would keep running unseen.
    if (current) await apiQuery("stopSession", { id: current.id }).catch((e) => setError(translate("hosted.stopFailed", { error: String(e) })));
  }, [session]);

  return { session, busyLab, error, launch, stop };
}
