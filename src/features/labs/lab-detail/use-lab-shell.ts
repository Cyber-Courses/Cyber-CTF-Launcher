"use client";

import { useState } from "react";
import { terminalWindow, type Runtime } from "@/lib/tauri";
import { describeError } from "@/lib/errors";

/** Opens a shell into the lab's attack box in the app's own shell window (it offers the system
 *  terminal too). `runtime` is the lab's. Keeps why it failed. */
export function useLabShell(labId: string, title: string, runtime: Runtime) {
  const [error, setError] = useState<string | null>(null);
  const open = () => {
    setError(null);
    terminalWindow(labId, runtime, title).catch((e: unknown) => {
      setError(describeError(e));
    });
  };
  return { open, error };
}
