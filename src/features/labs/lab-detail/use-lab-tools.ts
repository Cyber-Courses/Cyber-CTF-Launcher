"use client";

import { useEffect, useState } from "react";
import { labTools, type LabTool, type Runtime } from "@/lib/tauri";
import { warn } from "@/lib/failure";

/** The lab's observers (`tools:` in its spec), read once it runs: static addresses, so the
 *  last answer stays good across a stop and a start. `runtime` is the lab's declared one: the
 *  backend reads the form it was started in (a container lab run as one VM per machine has VM
 *  addresses). */
export function useLabTools(labId: string, running: boolean, runtime: Runtime): LabTool[] {
  const [tools, setTools] = useState<LabTool[]>([]);
  useEffect(() => {
    if (!running) return;
    let alive = true;
    labTools(labId, runtime)
      .then((found) => {
        if (alive) setTools(found);
      })
      .catch(
        warn("reading the lab's tools", () => {
          if (alive) setTools([]);
        }),
      );
    return () => {
      alive = false;
    };
  }, [labId, running, runtime]);
  return tools;
}
