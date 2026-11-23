"use client";

import { useEffect, useState, type ReactNode } from "react";
import { loadSettings } from "@/lib/settings-store";

/** Its children once this window has the player's settings from the core (`loadSettings`, which
 *  first hands over what an earlier version kept in the webview's storage), so nothing reads a
 *  default the player changed. A core that can't answer leaves the defaults (logged). */
export function SettingsGate({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    let live = true;
    void loadSettings().then(() => {
      if (live) setReady(true);
    });
    return () => {
      live = false;
    };
  }, []);
  return ready ? children : null;
}
