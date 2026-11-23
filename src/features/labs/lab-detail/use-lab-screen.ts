"use client";

import { useState } from "react";
import { screenOpenNative, screenWindow, type Runtime } from "@/lib/tauri";
import { describeError } from "@/lib/errors";

/** Opens a lab machine's screen in its own window (its graphical console), or the VM in its
 *  hypervisor's own app. `runtime` is the lab's. Keeps why it failed. */
export function useLabScreen(labId: string, title: string, runtime: Runtime) {
  const [error, setError] = useState<string | null>(null);
  const open = (machine: string) => {
    setError(null);
    screenWindow(labId, runtime, machine, title).catch((e: unknown) => {
      setError(describeError(e));
    });
  };
  const openNative = (machine: string) => {
    setError(null);
    screenOpenNative(labId, runtime, machine).catch((e: unknown) => {
      setError(describeError(e));
    });
  };
  return { open, openNative, error };
}
