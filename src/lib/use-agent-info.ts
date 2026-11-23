"use client";

import { useRef, useState } from "react";
import { agentInfo, type AgentInfo } from "@/lib/tauri";
import { warn } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";

/** How often this machine's agent is read again while signed in: it registers, goes online, and
 *  registers again for another account, in the background. */
const AGENT_POLL_MS = 5_000;

/** This machine's agent as the last read found it, or why that read failed (`undefined`: no
 *  failure). Both unset until the first read answers, and while signed out. */
export type AgentRead = { value: AgentInfo | null; error?: unknown };

/** This machine's agent while `loggedIn`, read again as it registers. A failed read keeps the last
 *  answer and is logged once, not on every poll that repeats it. */
export function useAgentInfo(loggedIn: boolean): AgentRead {
  const [agent, setAgent] = useState<AgentRead>({ value: null });
  const failing = useRef(false);
  usePoll(agentInfo, AGENT_POLL_MS, {
    enabled: loggedIn,
    onValue: (value) => {
      failing.current = false;
      setAgent({ value });
    },
    onError: (error) => {
      if (!failing.current) warn("reading this machine's agent")(error);
      failing.current = true;
      setAgent((a) => ({ value: a.value, error }));
    },
  });
  return loggedIn ? agent : { value: null };
}
