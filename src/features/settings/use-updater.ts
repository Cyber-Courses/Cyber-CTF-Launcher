"use client";

import { useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { restartApp } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { installUpdate } from "@/lib/install-update";
import { translate } from "@/lib/i18n";

export type UpdateState = { phase: "idle" | "checking" | "none" | "error" } | { phase: "available" | "installing" | "installed"; update: Update };

/**
 * The About section's update check: ask the update server, then download and install what it
 * offers ([`installUpdate`]: not while a lab is deploying) and reopen on the new version. `checkedAt` is when the
 * last check ended.
 */
export function useUpdater() {
  const [upd, setUpd] = useState<UpdateState>({ phase: "idle" });
  const [checkedAt, setCheckedAt] = useState<number | null>(null);

  async function checkNow() {
    setUpd({ phase: "checking" });
    try {
      const u = await check();
      setUpd(u ? { phase: "available", update: u } : { phase: "none" });
    } catch {
      setUpd({ phase: "error" });
    }
    setCheckedAt(Date.now());
  }

  async function installNow() {
    if (!("update" in upd)) return;
    const { update } = upd;
    const installed = await installUpdate(update, () => {
      setUpd({ phase: "installing", update });
    });
    setUpd({ phase: installed ? "installed" : "available", update });
    if (!installed) return;
    // Reopen on the new version straight away; Restart now stays as the fallback.
    await restartApp().catch(tell(translate("settings.about.restartFailed")));
  }

  // Click handlers: each step says how it went in `upd`, so nothing is left to reject to.
  const checkUpdates = () => {
    void checkNow();
  };
  const install = () => {
    void installNow();
  };
  return { upd, checkedAt, checkUpdates, install };
}
