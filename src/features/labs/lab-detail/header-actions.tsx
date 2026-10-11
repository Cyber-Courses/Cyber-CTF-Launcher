"use client";

import { ExternalLink, LogIn, Pause, Play, Power, Square, type LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Tip } from "@/components/ui/tip";
import { StartTimer } from "@/features/labs/lab-timers";
import type { HeaderMode, LabAction, LabState } from "@/features/labs/lab-detail/lab-state";
import { openExternal, tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** A button's icon and word, or a spinner and its busy word while its action runs. */
function ActionLabel({ pending, icon: Icon, idle, busy }: { pending: boolean; icon: LucideIcon; idle: string; busy: string }) {
  return pending ? (
    <>
      <Spinner className="size-3.5" /> {busy}
    </>
  ) : (
    <>
      <Icon className="size-3.5" /> {idle}
    </>
  );
}

/**
 * The header's actions for a lab that is up, parked, starting, stopping or interrupted, or
 * logged out ("start" mode is StartLabButton): the one thing to do next, and the others.
 */
export function HeaderActions({
  mode,
  state: s,
  url,
  busy,
  deploying = false,
  acting,
  resetting,
  onAct,
  onRemove,
  onStop,
  onLogin,
}: {
  mode: Exclude<HeaderMode, "start">;
  state: LabState;
  url: string | null | undefined;
  busy: boolean;
  /** An operation the backend reports in flight on the lab (one started before a window reload,
   *  so not this session's `busy`): every action waits for it. */
  deploying?: boolean;
  acting: LabAction | null;
  resetting: boolean;
  onAct: (what: LabAction) => void;
  /** Asks to confirm, then stops and removes the lab. */
  onRemove: () => void;
  onStop: () => Promise<unknown> | void;
  onLogin?: () => Promise<void>;
}) {
  const t = useT();
  const locked = busy || deploying;
  switch (mode) {
    case "running":
      return (
        <>
          {url && (
            <Button variant="outline" onClick={() => openExternal(url)} title={t("labs.detail.openUrl", { url })}>
              <ExternalLink className="size-3.5" /> {t("labs.detail.openInBrowser")}
            </Button>
          )}
          {s.canPause && (
            <Tip key="pause" text={t("labs.detail.pauseHint")}>
              <Button variant="outline" onClick={() => onAct("pause")} disabled={locked}>
                <ActionLabel pending={acting === "pause"} icon={Pause} idle={t("labs.detail.pause")} busy={t("labs.detail.pausing")} />
              </Button>
            </Tip>
          )}
          {s.canShutdown && (
            <Tip key="shutdown" text={t("labs.detail.shutdownHint")}>
              <Button variant="outline" onClick={() => onAct("shutdown")} disabled={locked}>
                <ActionLabel pending={acting === "shutdown"} icon={Power} idle={t("labs.detail.shutDown")} busy={t("labs.detail.shuttingDown")} />
              </Button>
            </Tip>
          )}
          <Tip key="remove" text={t("labs.detail.removeHint")}>
            <Button variant="destructive" onClick={onRemove} disabled={locked}>
              {s.operation === "stop" && !resetting ? (
                t("labs.detail.stopping")
              ) : (
                <>
                  <Square className="size-3.5" /> {t("labs.detail.stopRemove")}
                </>
              )}
            </Button>
          </Tip>
        </>
      );
    case "parked":
      return (
        <>
          <Tip key="remove-parked" text={t("labs.detail.removeHint")}>
            <Button variant="destructive" onClick={onRemove} disabled={locked}>
              <Square className="size-3.5" /> {t("labs.detail.stopRemove")}
            </Button>
          </Tip>
          <Tip key="resume" text={t("labs.detail.resumeHint")}>
            <Button variant="primary" onClick={() => onAct("resume")} disabled={locked}>
              <ActionLabel pending={busy} icon={Play} idle={t("labs.detail.resume")} busy={t("labs.detail.resuming")} />
            </Button>
          </Tip>
        </>
      );
    case "stopping":
      return (
        <Button variant="destructive" disabled>
          <Spinner className="size-3.5" /> {t("labs.detail.stopping")}
        </Button>
      );
    case "starting":
      return (
        <Button variant="primary" disabled>
          <Spinner className="size-3.5" /> {t("labs.detail.starting")} <StartTimer />
        </Button>
      );
    case "interrupted":
      // Machines exist but nothing is deploying and the lab isn't fully up: a previous run was
      // interrupted (e.g. the app restarted mid-start). Clean it up before a fresh start.
      return (
        <Button variant="destructive" onClick={() => onStop()} disabled={locked} title={t("labs.detail.interruptedHint")}>
          {busy ? (
            t("labs.detail.cleaningUp")
          ) : (
            <>
              <Square className="size-3.5" /> {t("labs.detail.stopCleanUp")}
            </>
          )}
        </Button>
      );
    case "signIn":
      // Logged out: say so on the button and log in from it, rather than a greyed-out Start.
      return (
        <Button variant="primary" onClick={() => void onLogin?.().catch(tell(t("labs.detail.signInFailed")))}>
          <LogIn className="size-3.5" /> {t("labs.detail.signInToStart")}
        </Button>
      );
  }
}
