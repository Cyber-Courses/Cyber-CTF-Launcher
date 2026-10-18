"use client";

import { ExternalLink, Pause, Play, Power, Square, type LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Tip } from "@/components/ui/tip";
import type { LabAction, LabState } from "@/features/labs/lab-detail/lab-state";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

// The header's actions for a lab whose machines are built: up (open, pause, shut down, remove)
// or parked (remove, resume).

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

/** A lab that is up: open it, pause or shut it down (when it can), stop and remove it. */
export function RunningActions({
  state: s,
  url,
  locked,
  acting,
  resetting,
  onAct,
  onRemove,
}: {
  state: LabState;
  url: string | null | undefined;
  /** Every action waits for the operation in flight. */
  locked: boolean;
  acting: LabAction | null;
  resetting: boolean;
  onAct: (what: LabAction) => void;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    <>
      {url && (
        <Button
          variant="outline"
          onClick={() => {
            openExternal(url);
          }}
          title={t("labs.detail.openUrl", { url })}
        >
          <ExternalLink className="size-3.5" /> {t("labs.detail.openInBrowser")}
        </Button>
      )}
      {s.canPause && <ParkButton key="pause" what="pause" icon={Pause} locked={locked} acting={acting} onAct={onAct} />}
      {s.canShutdown && <ParkButton key="shutdown" what="shutdown" icon={Power} locked={locked} acting={acting} onAct={onAct} />}
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
}

/** The words of Pause and Shut down: the hint, the button, and the button while it runs. */
const PARK_WORDS = {
  pause: { hint: "labs.detail.pauseHint", idle: "labs.detail.pause", busy: "labs.detail.pausing" },
  shutdown: { hint: "labs.detail.shutdownHint", idle: "labs.detail.shutDown", busy: "labs.detail.shuttingDown" },
} as const;

/** Pause or Shut down a lab that is up, with its hint; a spinner while that one runs. */
function ParkButton({
  what,
  icon,
  locked,
  acting,
  onAct,
}: {
  what: "pause" | "shutdown";
  icon: LucideIcon;
  locked: boolean;
  acting: LabAction | null;
  onAct: (what: LabAction) => void;
}) {
  const t = useT();
  const words = PARK_WORDS[what];
  return (
    <Tip text={t(words.hint)}>
      <Button
        variant="outline"
        onClick={() => {
          onAct(what);
        }}
        disabled={locked}
      >
        <ActionLabel pending={acting === what} icon={icon} idle={t(words.idle)} busy={t(words.busy)} />
      </Button>
    </Tip>
  );
}

/** A paused or shut-down lab: remove it, or resume it as it was. */
export function ParkedActions({ locked, busy, onAct, onRemove }: { locked: boolean; busy: boolean; onAct: (what: LabAction) => void; onRemove: () => void }) {
  const t = useT();
  return (
    <>
      <Tip key="remove-parked" text={t("labs.detail.removeHint")}>
        <Button variant="destructive" onClick={onRemove} disabled={locked}>
          <Square className="size-3.5" /> {t("labs.detail.stopRemove")}
        </Button>
      </Tip>
      <Tip key="resume" text={t("labs.detail.resumeHint")}>
        <Button
          variant="primary"
          onClick={() => {
            onAct("resume");
          }}
          disabled={locked}
        >
          <ActionLabel pending={busy} icon={Play} idle={t("labs.detail.resume")} busy={t("labs.detail.resuming")} />
        </Button>
      </Tip>
    </>
  );
}
