"use client";

import { useState } from "react";
import { dockerStartEngine, dockerUseEngine, type DockerEngine } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** Switch Docker to an engine already running, or start the installed one that is stopped. Both
 *  re-read the machine when done. */
export function useEngineActions(onRefresh: () => void) {
  const t = useT();
  // Several engines can run at once; Docker talks to one. Switching = `docker context use`.
  const [switching, setSwitching] = useState(false);
  const switchTo = (engine: DockerEngine) => {
    setSwitching(true);
    dockerUseEngine(engine)
      .catch(tell(t("machine.errors.switchEngine")))
      .finally(() => {
        setSwitching(false);
        onRefresh();
      });
  };
  // Installed but stopped: start it from here instead of sending the player to do it.
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const start = (engine: DockerEngine) => {
    setStarting(true);
    setStartError(null);
    dockerStartEngine(engine)
      .catch((e) => setStartError(String(e)))
      .finally(() => {
        setStarting(false);
        onRefresh();
      });
  };
  return { switching, switchTo, starting, startError, start };
}
export type EngineActions = ReturnType<typeof useEngineActions>;
