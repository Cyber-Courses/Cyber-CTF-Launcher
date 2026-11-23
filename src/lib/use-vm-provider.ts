"use client";

import { useSyncExternalStore } from "react";
import { getVmProvider } from "@/lib/settings";
import { subscribeSettings } from "@/lib/settings-store";
import type { Provider } from "@/lib/tauri";

/** The hypervisor labs run on (Settings' choice, null = automatic), kept current as it changes
 *  anywhere in the app: in this window or another one, the Settings or the setup window (the
 *  core tells each window, `settings-store`). */
export function useVmProvider(): Provider | null {
  return useSyncExternalStore(subscribeSettings, getVmProvider, getVmProvider);
}
