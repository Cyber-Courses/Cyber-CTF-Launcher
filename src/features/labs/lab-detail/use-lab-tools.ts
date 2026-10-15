"use client";

import { useEffect, useState } from "react";
import { labTools, type LabTool } from "@/lib/tauri";

/** The lab's observers (`tools:` in its spec), read once it runs: static addresses, so the
 *  last answer stays good across a stop and a start. */
export function useLabTools(labId: string, running: boolean, isDocker: boolean): LabTool[] {
  const [tools, setTools] = useState<LabTool[]>([]);
  useEffect(() => {
    if (!running) return;
    let alive = true;
    labTools(labId, isDocker ? "DOCKER" : "VM")
      .then((found) => {
        if (alive) setTools(found);
      })
      .catch(() => {
        if (alive) setTools([]);
      });
    return () => {
      alive = false;
    };
  }, [labId, running, isDocker]);
  return tools;
}
