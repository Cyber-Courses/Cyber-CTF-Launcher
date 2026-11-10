"use client";

import { useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { warn } from "@/lib/failure";
import { installAndRestart } from "@/lib/install-update";

export type UpdateState = { phase: "idle" | "checking" | "none" | "error" } | { phase: "available" | "installing" | "installed"; update: Update };

/**
 * The About section's update check: ask the update server, then download and install what it
 * offers ([`installAndRestart`]: not while a lab is deploying) and reopen on the new version. `checkedAt` is when the
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
    } catch (e) {
      warn("checking for an update")(e);
      setUpd({ phase: "error" });
    }
    setCheckedAt(Date.now());
  }

  async function installNow() {
    if (!("update" in upd)) return;
    const { update } = upd;
    // Reopened on the new version straight away; Restart now stays as the fallback.
    const installed = await installAndRestart(
      update,
      () => {
        setUpd({ phase: "installing", update });
      },
      () => {
        setUpd({ phase: "installed", update });
      },
    );
    if (!installed) setUpd({ phase: "available", update });
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
