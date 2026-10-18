"use client";

import { useState } from "react";
import { useHostedLabs } from "@/features/hosted/use-hosted-labs";

/** Cyber CTF running the lab for the player: a hosted session with a public URL. */
export function useHostedRun(labId: string) {
  const hosted = useHostedLabs();
  // The active session belongs to this page only when it's for this lab (the hook may have
  // hydrated a session the player launched on another lab).
  const session = hosted.session && hosted.session.labId === labId ? hosted.session : null;
  const starting = hosted.busyLab === labId;
  // Show the panel after a start from this page, or whenever there's a live session for this lab.
  const [tried, setTried] = useState(false);
  return {
    session,
    starting,
    error: hosted.error,
    shown: !!session || (tried && (starting || !!hosted.error)),
    live: !!session && !["FAILED", "STOPPED", "EXPIRED"].includes(session.state),
    launch: () => {
      setTried(true);
      void hosted.launch(labId);
    },
    stop: () => {
      // Stopping a session keeps the panel up so a failed stop shows its error (a successful one
      // leaves nothing to show); dismissing a failed start or stop (no session) hides it.
      setTried(!!session);
      void hosted.stop();
    },
  };
}
