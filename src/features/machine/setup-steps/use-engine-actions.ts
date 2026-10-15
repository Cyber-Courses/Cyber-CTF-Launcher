"use client";

import { useState } from "react";
import { dockerStartEngine, dockerUseEngine, type DockerEngine } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

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
  // Kept with the engine it is about: another engine picked since doesn't show this one's failure.
  const [startFailure, setStartFailure] = useState<{ engine: DockerEngine; message: string } | null>(null);
  const startError = (engine: DockerEngine) => (startFailure?.engine === engine ? startFailure.message : null);
  const start = (engine: DockerEngine) => {
    setStarting(true);
    setStartFailure(null);
    dockerStartEngine(engine)
      .catch((e: unknown) => {
        setStartFailure({ engine, message: describeError(e) });
      })
      .finally(() => {
        setStarting(false);
        onRefresh();
      });
  };
  return { switching, switchTo, starting, startError, start };
}
export type EngineActions = ReturnType<typeof useEngineActions>;
