"use client";

import { type ButtonHTMLAttributes } from "react";
import { ExternalLink, LogIn, Wrench } from "lucide-react";
import { difficultyLabel, type Lab } from "@/features/labs/use-labs";
import { runtimeLabel } from "@/features/labs/lab-runtime";
import type { RowPhase } from "@/features/labs/lab-row-state";
import { RunPlaceIcons } from "@/features/labs/run-place-icons";
import { machineOpenSetup, serverOpenSetup, type LabStatus } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { LevelBadge } from "@/components/ui/badge";
import { StatusDot } from "@/components/ui/status-pill";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { openExternal, tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

// The cells of a lab's list row (see LabRow): its dot, title, columns, action and the stop dialog.

/** A row action: compact, and it doesn't open the row when clicked. */
function RowButton({
  danger = false,
  onClick,
  ...props
}: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onClick"> & { danger?: boolean; onClick: () => void }) {
  return (
    <Button
      size="xs"
      variant={danger ? "destructive" : "outline"}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      {...props}
    />
  );
}

/** The status dot: busy or starting (pulsing), running, solved (jewel) or idle, and its word. */
export function RowDot({ phase, running, solved, label }: { phase: RowPhase; running: boolean; solved: boolean; label: string | null }) {
  return (
    <span title={label ?? undefined} className="flex w-2 justify-center">
      {phase === "busy" || phase === "starting" ? (
        <StatusDot tone="warn" pulse />
      ) : running ? (
        <StatusDot tone="ok" />
      ) : solved ? (
        <span aria-hidden className="dot dot-jewel" />
      ) : (
        <StatusDot tone="muted" />
      )}
      <span className="sr-only">{label}</span>
    </span>
  );
}

/** The title over mono meta: slug, emulated, then what setup it needs or its description. */
export function RowTitle({ lab, emulated, setup, running }: { lab: Lab; emulated: boolean; setup: string | null; running: boolean }) {
  const t = useT();
  return (
    <span className="min-w-0">
      <span className="block truncate font-medium text-foreground">{lab.title}</span>
      <span className="block truncate font-mono text-[0.6875rem] text-faint">
        {lab.slug}
        {emulated && (
          <>
            {" · "}
            <span className="text-warning" title={t("labs.row.emulatedHint")}>
              {t("labs.row.emulated")}
            </span>
          </>
        )}
        {setup && !running ? (
          <>
            {" · "}
            <span className="text-warning">{setup}</span>
          </>
        ) : (
          ` · ${lab.description || lab.category}`
        )}
      </span>
    </span>
  );
}

/** The secondary columns (category, level, runtime with the run-place icons), folded away in a
 *  narrow container. */
export function RowColumns({
  lab,
  hostArch,
  emulates,
  running,
  status,
}: {
  lab: Lab;
  hostArch: string;
  emulates: boolean;
  running: boolean;
  status?: LabStatus;
}) {
  const t = useT();
  const rt = lab.runtime;
  return (
    <>
      <span className="hidden truncate font-mono text-[0.6875rem] text-faint @min-[44rem]:block">{lab.category}</span>
      <span className="hidden @min-[44rem]:block">
        {lab.difficulty > 0 && <LevelBadge level={lab.difficulty}>{difficultyLabel(t, lab.difficulty)}</LevelBadge>}
      </span>
      <span className="hidden items-center gap-2 font-mono text-[0.6875rem] text-faint @min-[44rem]:flex">
        {rt && (
          <>
            {/* Fixed to the longest label ("docker") so the place icons line up from row to row. */}
            <span className="w-[6ch] shrink-0">{runtimeLabel(rt)}</span>
            <RunPlaceIcons runtime={rt} hostArch={hostArch} emulates={emulates} running={running} status={status} />
          </>
        )}
      </span>
    </>
  );
}

/** The row action: Open and Stop while it runs, Resume when parked, setup or sign-in when needed. */
export function RowAction({
  status,
  running,
  parked,
  busy,
  doing,
  setup,
  connectCloud,
  loggedIn,
  onLogin,
  onStop,
  onResume,
}: {
  status?: LabStatus;
  running: boolean;
  parked: LabStatus["parked"] | false;
  busy: boolean;
  /** The operation in flight, as a word, while busy. */
  doing: string | null;
  setup: string | null;
  connectCloud: boolean;
  loggedIn: boolean;
  onLogin?: () => Promise<void>;
  /** Asks to confirm the stop. */
  onStop: () => void;
  onResume?: () => void;
}) {
  const t = useT();
  if (running)
    return (
      <>
        {status?.url && (
          <RowButton onClick={() => openExternal(status.url!)}>
            <ExternalLink className="size-3" /> {t("labs.row.openButton")}
          </RowButton>
        )}
        <RowButton danger onClick={onStop} disabled={busy}>
          {doing ?? t("labs.row.stop")}
        </RowButton>
      </>
    );
  if (parked)
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
  if (setup && connectCloud)
    return (
      <RowButton onClick={() => serverOpenSetup(null, "cloud").catch(tell(t("labs.row.connectFailed")))} title={t("labs.row.connectHint", { setup })}>
        <Wrench className="size-3" /> {t("labs.row.connect")}
      </RowButton>
    );
  if (setup)
    return (
      <RowButton onClick={() => machineOpenSetup().catch(tell(t("labs.row.setupFailed")))} title={t("labs.row.setupHint", { setup })}>
        <Wrench className="size-3" /> {t("labs.row.setUp")}
      </RowButton>
    );
  if (!loggedIn && onLogin)
    return (
      <RowButton onClick={() => void onLogin().catch(tell(t("labs.row.signInFailed")))} title={t("labs.row.signInHint")}>
        <LogIn className="size-3" /> {t("labs.row.signIn")}
      </RowButton>
    );
  return null;
}

/** Stop deletes the lab's machines: asked first, as the lab's own page does. */
export function StopDialog({ lab, status, onCancel, onConfirm }: { lab: Lab; status?: LabStatus; onCancel: () => void; onConfirm: () => void }) {
  const t = useT();
  return (
    // Clicks in the dialog stay in it: the row behind would open the lab.
    <div onClick={(e) => e.stopPropagation()}>
      <ConfirmDialog title={t("labs.row.removeTitle", { title: lab.title })} confirmLabel={t("labs.row.remove")} onCancel={onCancel} onConfirm={onConfirm}>
        {t(lab.runtime?.runtime === "CLOUD" ? "labs.row.removeBodyCloud" : "labs.row.removeBody")}
        {!status?.host && status?.place !== "local_vm" && t("labs.row.removeKeep")}
      </ConfirmDialog>
    </div>
  );
}
