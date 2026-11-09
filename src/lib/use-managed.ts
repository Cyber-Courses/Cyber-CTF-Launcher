"use client";

import { useEffect, useState } from "react";
import { managedSettings, type ManagedSettings } from "@/lib/tauri";
import { warn } from "@/lib/failure";

/** What this computer's administrator and package manager decide for the app (read once per
 *  mount: the policy is read once per run), or null until known or when it can't be read. */
export function useManagedSettings(): ManagedSettings | null {
  const [managed, setManaged] = useState<ManagedSettings | null>(null);
  useEffect(() => {
    managedSettings().then(setManaged).catch(warn("managed settings"));
  }, []);
  return managed;
}
