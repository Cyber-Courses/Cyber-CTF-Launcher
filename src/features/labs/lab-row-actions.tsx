"use client";

import { type ButtonHTMLAttributes } from "react";
import { ExternalLink, LogIn, Wrench } from "lucide-react";
import { machineOpenSetup, serverOpenSetup, type LabStatus } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { openExternal, tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

// A lab list row's action (see LabRow).

/** A row action: compact, drawn above the title's stretched click area (see LabRow). */
function RowButton({ danger = false, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { danger?: boolean }) {
  return <Button size="xs" variant={danger ? "destructive" : "outline"} {...props} />;
}

type RowActionProps = {
  status?: LabStatus;
  running: boolean;
  parked: LabStatus["parked"] | false;
  busy: boolean;
  /** The operation in flight, as a word, while busy. */
  doing: string | null;
  setup: string | null;
  connectCloud: boolean;
  loggedIn: boolean;
  onLogin?: () => Promise<boolean>;
  /** Once the sign-in the row started went through: go on with what the row is for. */
  onSignedIn?: () => void;
  /** Asks to confirm the stop. */
  onStop: () => void;
  onResume?: () => void;
};

/** The row action: Open and Stop while it runs, Resume when parked, setup or sign-in when needed. */
export function RowAction(p: RowActionProps) {
  const t = useT();
  if (p.running) return <RunningActions url={p.status?.url ?? null} busy={p.busy} doing={p.doing} onStop={p.onStop} />;
  if (p.parked) return <ParkedActions parked={p.parked} busy={p.busy} doing={p.doing} onResume={p.onResume} />;
  if (p.setup) return <SetupAction setup={p.setup} connectCloud={p.connectCloud} />;
  const { onLogin, onSignedIn } = p;
  if (!p.loggedIn && onLogin)
    return (
      <RowButton
        onClick={() => {
          onLogin()
            .then((signedIn) => {
              if (signedIn) onSignedIn?.();
            })
            .catch(tell(t("labs.row.signInFailed")));
        }}
        title={t("labs.row.signInHint")}
      >
        <LogIn className="size-3" /> {t("labs.row.signIn")}
      </RowButton>
    );
  return null;
}

/** A running lab: Open (when it has a URL) and Stop. */
function RunningActions({ url, busy, doing, onStop }: { url: string | null; busy: boolean; doing: string | null; onStop: () => void }) {
  const t = useT();
  return (
    <>
      {url && (
        <RowButton
          onClick={() => {
            openExternal(url);
          }}
        >
          <ExternalLink className="size-3" /> {t("labs.row.openButton")}
        </RowButton>
      )}
      <RowButton danger onClick={onStop} disabled={busy}>
        {doing ?? t("labs.row.stop")}
      </RowButton>
    </>
  );
}

/** A paused or shut-down lab: how it was left, and Resume. */
function ParkedActions({
  parked,
  busy,
  doing,
  onResume,
}: {
  parked: NonNullable<LabStatus["parked"]>;
  busy: boolean;
  doing: string | null;
  onResume?: () => void;
}) {
  const t = useT();
  return (
    <>
      <span className="hidden font-mono text-[0.6875rem] text-faint @min-[30rem]:inline">
        {parked === "pause" ? t("labs.row.paused") : t("labs.row.shutDown")}
      </span>
      {onResume && (
        <RowButton onClick={onResume} disabled={busy} title={t("labs.row.resumeHint")}>
          {doing ?? t("labs.row.resume")}
        </RowButton>
      )}
    </>
  );
}

/** What this machine is missing: connect a cloud account, or set the machine up. */
function SetupAction({ setup, connectCloud }: { setup: string; connectCloud: boolean }) {
  const t = useT();
  if (connectCloud)
    return (
      <RowButton
        onClick={() => {
          serverOpenSetup(null, "cloud").catch(tell(t("labs.row.connectFailed")));
        }}
        title={t("labs.row.connectHint", { setup })}
      >
        <Wrench className="size-3" /> {t("labs.row.connect")}
      </RowButton>
    );
  return (
    <RowButton
      onClick={() => {
        machineOpenSetup().catch(tell(t("labs.row.setupFailed")));
      }}
      title={t("labs.row.setupHint", { setup })}
    >
      <Wrench className="size-3" /> {t("labs.row.setUp")}
    </RowButton>
  );
}
