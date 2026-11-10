"use client";

import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Button } from "@/components/ui/button";
import { managedSettings } from "@/lib/tauri";
import { installAndRestart, restartOnUpdate } from "@/lib/install-update";
import { useDeployingLabs } from "@/features/labs/runs/operations";
import { cn } from "@/lib/utils";
import { ignore } from "@/lib/failure";
import { useT } from "@/lib/i18n";

type Phase = "idle" | "downloading" | "ready" | "error";

/** The update found on launch (null: none, or no check in a dev build), where its install is,
 *  and the install itself. */
function useUpdate() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");

  useEffect(() => {
    // A dev build carries its branch's version, so it would always "find" the latest release.
    if (process.env.NODE_ENV === "development") return;
    // A copy the organization or a package manager updates has no in-app updater to ask.
    managedSettings()
      .then((m) => (m.updates === "inApp" ? check() : null))
      .then((u) => {
        if (u) setUpdate(u);
      })
      .catch(ignore("offline, or no update endpoint: the banner stays hidden; Settings' check says why"));
  }, []);

  async function install() {
    if (!update) return;
    // Reopened on the new version straight away; Restart now stays as the fallback if it can't
    // (the update is installed: a failed relaunch is not a failed update).
    const installed = await installAndRestart(
      update,
      () => {
        setPhase("downloading");
      },
      () => {
        setPhase("ready");
      },
    );
    // Only a download that began and failed is the banner's error; a wait for the deploys
    // leaves it as it was.
    if (!installed) setPhase((p) => (p === "downloading" ? "error" : p));
  }
  return { update, phase, install };
}

/**
 * Checks for an update on launch (via plugins.updater) and offers a one-click install.
 * Silent when up to date, when the check fails (offline), in dev builds, and when updates come from
 * elsewhere (the organization or a package manager: `managedSettings`). After install the app relaunches
 * itself through a tiny command (no process plugin needed); Restart now is the fallback.
 */
export function UpdateBanner() {
  const t = useT();
  const { update, phase, install } = useUpdate();
  // Installing quits every Cyber CTF process, the deploy workers included (the installer ends
  // them by name on Windows), so an update waits for running deploys to finish.
  const deploying = useDeployingLabs().size > 0;

  if (!update) return null;

  return (
    <div role="status" className="flex h-10 shrink-0 items-center gap-2.5 border-b border-border bg-jewel/[0.06] px-6 text-[0.8125rem]">
      <span aria-hidden className={cn("size-1.5 shrink-0 rounded-full", phase === "error" ? "bg-destructive" : "bg-jewel")} />
      <span className="min-w-0 flex-1 truncate text-foreground">
        <UpdateMessage phase={phase} deploying={deploying} version={update.version} />
      </span>
      {phase === "ready" ? (
        <Button
          variant="outline"
          size="xs"
          onClick={() => {
            void restartOnUpdate();
          }}
        >
          {t("shell.update.restartNow")}
        </Button>
      ) : (
        <Button
          variant="outline"
          size="xs"
          onClick={() => {
            void install();
          }}
          disabled={phase === "downloading" || deploying}
        >
          {phase === "downloading" ? t("shell.update.updating") : t("shell.update.update")}
        </Button>
      )}
    </div>
  );
}

/** What the banner says: installed, failed, waiting for the deploys, or the version on offer. */
function UpdateMessage({ phase, deploying, version }: { phase: Phase; deploying: boolean; version: string }) {
  const t = useT();
  if (phase === "ready") return t("shell.update.installed");
  if (phase === "error") return t("shell.update.failed");
  if (deploying && phase === "idle") return t("shell.update.waitForDeploys");
  return t.rich("shell.update.available", { version: (v) => <span className="font-mono text-[0.75rem]">{v}</span> }, { version });
}
