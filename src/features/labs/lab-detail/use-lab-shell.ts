"use client";

import { useState } from "react";
import { terminalWindow } from "@/lib/tauri";
import { describeError } from "@/lib/errors";

/** Opens a shell into the lab in the app's own shell window (it offers the system terminal too):
 *  a container or remote lab's own shell; a local VM lab's attack VM. Keeps why it failed. */
export function useLabShell(labId: string, title: string, inLab: boolean) {
  const [error, setError] = useState<string | null>(null);
  const open = () => {
    setError(null);
    terminalWindow(labId, inLab ? "lab" : "attackVm", inLab ? "DOCKER" : "VM", title).catch((e) => setError(describeError(e)));
  };
  return { open, error };
}
