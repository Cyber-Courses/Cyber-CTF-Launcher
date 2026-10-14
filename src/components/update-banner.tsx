"use client";

import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Button } from "@/components/ui/button";
import { deployingLabs, restartApp } from "@/lib/tauri";
import { useDeployingLabs } from "@/lib/deploy-store";
import { cn } from "@/lib/utils";
import { ignore, tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/**
 * Checks for an update on launch (via plugins.updater) and offers a one-click install.
 * Silent when up to date, when the check fails (offline), and in dev builds. After install the app relaunches
 * itself through a tiny command (no process plugin needed); Restart now is the fallback.
 */
export function UpdateBanner() {
  const t = useT();
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<"idle" | "downloading" | "ready" | "error">("idle");
  // Installing quits every Cyber CTF process, the deploy workers included (the installer ends
  // them by name on Windows), so an update waits for running deploys to finish.
  const deploying = useDeployingLabs().size > 0;

  useEffect(() => {
    // A dev build carries its branch's version, so it would always "find" the latest release.
    if (process.env.NODE_ENV === "development") return;
    check()
      .then((u) => {
        if (u) setUpdate(u);
      })
      .catch(() => {
        /* offline, dev, or no update endpoint: stay silent */
      });
  }, []);

  if (!update) return null;

  async function install() {
    if (!update) return;
    // Asked again now: the polled set can be a few seconds old.
    if ((await deployingLabs().catch(() => [])).length > 0) return;
    setPhase("downloading");
    try {
      await update.downloadAndInstall();
    } catch {
      setPhase("error");
      return;
    }
    setPhase("ready");
    // Reopen on the new version straight away; the button stays as a fallback if it can't
    // (the update is installed: a failed relaunch is not a failed update).
    await restartApp().catch(ignore("Restart now stays as the fallback"));
  }

  return (
    <div role="status" className="flex h-10 shrink-0 items-center gap-2.5 border-b border-border bg-jewel/[0.06] px-6 text-[0.8125rem]">
      <span aria-hidden className={cn("size-1.5 shrink-0 rounded-full", phase === "error" ? "bg-destructive" : "bg-jewel")} />
      <span className="min-w-0 flex-1 truncate text-foreground">
        {phase === "ready"
          ? t("shell.update.installed")
          : phase === "error"
            ? t("shell.update.failed")
            : deploying && phase === "idle"
              ? t("shell.update.waitForDeploys")
              : t.rich("shell.update.available", { version: (v) => <span className="font-mono text-[0.75rem]">{v}</span> }, { version: update.version })}
      </span>
      {phase === "ready" ? (
        <Button variant="outline" size="xs" onClick={() => restartApp().catch(tell(t("shell.failures.restart")))}>
          {t("shell.update.restartNow")}
        </Button>
      ) : (
        <Button variant="outline" size="xs" onClick={install} disabled={phase === "downloading" || deploying}>
          {phase === "downloading" ? t("shell.update.updating") : t("shell.update.update")}
        </Button>
      )}
    </div>
  );
}
