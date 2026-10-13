"use client";

import { useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { deployingLabs, restartApp } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { translate } from "@/lib/i18n";

export type UpdateState = { phase: "idle" | "checking" | "none" | "error" } | { phase: "available" | "installing" | "installed"; update: Update };

/**
 * The About section's update check: ask the update server, then download and install what it
 * offers (unless a lab is deploying) and reopen on the new version. `checkedAt` is when the
 * last check ended.
 */
export function useUpdater() {
  const [upd, setUpd] = useState<UpdateState>({ phase: "idle" });
  const [checkedAt, setCheckedAt] = useState<number | null>(null);

  async function checkUpdates() {
    setUpd({ phase: "checking" });
    try {
      const u = await check();
      setUpd(u ? { phase: "available", update: u } : { phase: "none" });
    } catch {
      setUpd({ phase: "error" });
    }
    setCheckedAt(Date.now());
  }

  async function install() {
    if (!("update" in upd)) return;
    const { update } = upd;
    // Installing quits the deploy workers too: asked again now, as the polled set can be a few
    // seconds old.
    if ((await deployingLabs().catch(() => [])).length > 0) return;
    setUpd({ phase: "installing", update });
    try {
      await update.downloadAndInstall();
      setUpd({ phase: "installed", update });
    } catch {
      setUpd({ phase: "available", update });
      return;
    }
    // Reopen on the new version straight away; Restart now stays as the fallback.
    await restartApp().catch(tell(translate("settings.about.restartFailed")));
  }

  return { upd, checkedAt, checkUpdates, install };
}
