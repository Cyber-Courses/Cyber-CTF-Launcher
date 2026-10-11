"use client";

import { useState, type ButtonHTMLAttributes } from "react";
import { ChevronRight, ExternalLink, LogIn, Wrench } from "lucide-react";
import { difficultyLabel, type Lab } from "@/features/labs/use-labs";
import { runsNatively, runtimeLabel } from "@/features/labs/lab-runtime";
import { rowPhase, type RowPhase } from "@/features/labs/lab-row-state";
import { RunPlaceIcons } from "@/features/labs/run-place-icons";
import { machineOpenSetup, serverOpenSetup, type LabStatus } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { LevelBadge } from "@/components/ui/badge";
import { StatusDot } from "@/components/ui/status-pill";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { OPERATION_STATUS } from "@/lib/deploy-store";
import { openExternal, tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

// Home imports it from here, beside the row it feeds.
export { emulatorReady } from "@/features/labs/lab-runtime";

const PHASE_WORDS = {
  starting: "labs.state.startingEllipsis",
  running: "labs.state.running",
  paused: "labs.state.paused",
  shutDown: "labs.state.shutDown",
  solved: "labs.state.solved",
  notStarted: "labs.state.notStarted",
} as const satisfies Record<Exclude<RowPhase, "busy">, string>;

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

/**
 * One lab as a dense 3.25rem list row: a status dot, the title over mono meta, then mono columns
 * (category, level, runtime with the run-place icons), the row action (Open and Stop while it
 * runs, Resume when parked, setup or sign-in when needed) and a chevron into the detail
 * (starting happens on the lab's own page). The secondary columns fold away in a narrow
 * container. Reused on the Overview rail-list and the full Labs list.
 */
export function LabRow({
  lab,
  status,
  busy = false,
  operation,
  loggedIn,
  onLogin,
  hostArch,
  emulates = false,
  onOpen,
  onStop,
  onResume,
  solved = false,
  setup = null,
  connectCloud = false,
  deploying = false,
}: {
  lab: Lab;
  status?: LabStatus;
  busy?: boolean;
  /** The operation in flight while `busy` (launch, stop, shutdown…), for its label. */
  operation?: string;
  loggedIn: boolean;
  /** Logged out: the start button logs in instead of being greyed out. */
  onLogin?: () => Promise<void>;
  hostArch: string;
  /** An emulator (QEMU) is ready here, so a VM lab built for another CPU can run, slowly. */
  emulates?: boolean;
  onOpen: () => void;
  onStop: () => void;
  /** Brings a paused or shut-down lab back as it was. */
  onResume?: () => void;
  /** The player already solved it (website evidence). */
  solved?: boolean;
  /** What this machine is missing to run it (see lab-readiness), or null. */
  setup?: string | null;
  /** A cloud lab with no account of its cloud yet: the setup action connects one. */
  connectCloud?: boolean;
  /** A deploy is running for it in the backend (e.g. started before a reload), for the dot. */
  deploying?: boolean;
}) {
  const t = useT();
  const rt = lab.runtime;
  // Unknown host (report not in yet) or a lab for any CPU: native, never a wrong "emulated".
  const native = !hostArch || !rt || runsNatively(rt, hostArch);
  // Emulated only where it actually runs here: containers always, VMs with an emulator ready.
  const emulated = !native && (rt?.runtime !== "VM" || emulates);
  const running = status?.running ?? false;
  const parked = !running && (status?.parked ?? null);
  // While busy, what is actually happening: a lab being started reports running long before
  // the launch is done, so "Stopping…" there would be wrong.
  const doing = busy ? `${(operation && OPERATION_STATUS[operation]) ?? t("labs.state.working")}…` : null;
  // Stop deletes the lab's machines: ask first, as the lab's own page does.
  const [confirmingStop, setConfirmingStop] = useState(false);
  const phase = rowPhase({ status, busy, deploying, solved });
  const stateLabel = phase === "busy" ? doing : t(PHASE_WORDS[phase]);

  return (
    <div className="@container border-t border-border first:border-t-0">
      <div
        role="button"
        tabIndex={0}
        aria-label={t("labs.row.open", { title: lab.title })}
        onClick={onOpen}
        onKeyDown={(e) => {
          // Only the row itself: Enter on one of its buttons (or in its dialog) is theirs.
          if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
          e.preventDefault();
          onOpen();
        }}
        className="group grid h-13 cursor-pointer grid-cols-[auto_minmax(0,1fr)_auto_1rem] items-center gap-3.5 px-4 text-[0.8125rem] outline-none transition-colors hover:bg-glass focus-visible:bg-glass focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring @min-[44rem]:grid-cols-[auto_minmax(0,1fr)_8rem_5.5rem_7rem_9.5rem_1rem]"
      >
        <span title={stateLabel ?? undefined} className="flex w-2 justify-center">
          {phase === "busy" || phase === "starting" ? (
            <StatusDot tone="warn" pulse />
          ) : running ? (
            <StatusDot tone="ok" />
          ) : solved ? (
            <span aria-hidden className="dot dot-jewel" />
          ) : (
            <StatusDot tone="muted" />
          )}
          <span className="sr-only">{stateLabel}</span>
        </span>

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

        <span className="flex items-center justify-end gap-1.5">
          {running ? (
            <>
              {status?.url && (
                <RowButton onClick={() => openExternal(status.url!)}>
                  <ExternalLink className="size-3" /> {t("labs.row.openButton")}
                </RowButton>
              )}
              <RowButton danger onClick={() => setConfirmingStop(true)} disabled={busy}>
                {doing ?? t("labs.row.stop")}
              </RowButton>
            </>
          ) : parked ? (
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
          ) : setup && connectCloud ? (
            <RowButton onClick={() => serverOpenSetup(null, "cloud").catch(tell(t("labs.row.connectFailed")))} title={t("labs.row.connectHint", { setup })}>
              <Wrench className="size-3" /> {t("labs.row.connect")}
            </RowButton>
          ) : setup ? (
            <RowButton onClick={() => machineOpenSetup().catch(tell(t("labs.row.setupFailed")))} title={t("labs.row.setupHint", { setup })}>
              <Wrench className="size-3" /> {t("labs.row.setUp")}
            </RowButton>
          ) : !loggedIn && onLogin ? (
            <RowButton onClick={() => void onLogin().catch(tell(t("labs.row.signInFailed")))} title={t("labs.row.signInHint")}>
              <LogIn className="size-3" /> {t("labs.row.signIn")}
            </RowButton>
          ) : null}
          {/* Starting happens on the lab's own page, where you pick where to run; the row opens it. */}
        </span>

        <ChevronRight className="size-4 text-faint transition-colors group-hover:text-foreground" />
      </div>

      {confirmingStop && (
        // Clicks in the dialog stay in it: the row behind would open the lab.
        <div onClick={(e) => e.stopPropagation()}>
          <ConfirmDialog
            title={t("labs.row.removeTitle", { title: lab.title })}
            confirmLabel={t("labs.row.remove")}
            onCancel={() => setConfirmingStop(false)}
            onConfirm={() => {
              setConfirmingStop(false);
              onStop();
            }}
          >
            {t(rt?.runtime === "CLOUD" ? "labs.row.removeBodyCloud" : "labs.row.removeBody")}
            {!status?.host && status?.place !== "local_vm" && t("labs.row.removeKeep")}
          </ConfirmDialog>
        </div>
      )}
    </div>
  );
}
