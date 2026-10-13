"use client";

import { memo, useState } from "react";
import { ChevronRight } from "lucide-react";
import type { Lab } from "@/features/labs/use-labs";
import { rowFacts, rowPhase, type RowPhase } from "@/features/labs/lab-row-state";
import { RowAction, RowColumns, RowDot, RowTitle, StopDialog } from "@/features/labs/lab-row-parts";
import type { LabStatus } from "@/lib/tauri";
import { OPERATION_STATUS } from "@/lib/deploy-store";
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

/**
 * One lab as a dense 3.25rem list row: a status dot, the title over mono meta, then mono columns
 * (category, level, runtime with the run-place icons), the row action (Open and Stop while it
 * runs, Resume when parked, setup or sign-in when needed) and a chevron into the detail
 * (starting happens on the lab's own page). The secondary columns fold away in a narrow
 * container. Reused on the Overview rail-list and the full Labs list. Memoized: the handlers take
 * the lab, so a list passes the same ones to every row and a poll re-renders only the rows whose
 * lab changed.
 */
export const LabRow = memo(function LabRow({
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
  onOpen: (lab: Lab) => void;
  onStop: (lab: Lab) => void;
  /** Brings a paused or shut-down lab back as it was. */
  onResume?: (lab: Lab) => void;
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
  const { emulated, running, parked } = rowFacts({ runtime: lab.runtime, status, hostArch, emulates });
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
        onClick={() => onOpen(lab)}
        onKeyDown={(e) => {
          // Only the row itself: Enter on one of its buttons (or in its dialog) is theirs.
          if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
          e.preventDefault();
          onOpen(lab);
        }}
        className="group grid h-13 cursor-pointer grid-cols-[auto_minmax(0,1fr)_auto_1rem] items-center gap-3.5 px-4 text-[0.8125rem] outline-none transition-colors hover:bg-glass focus-visible:bg-glass focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring @min-[44rem]:grid-cols-[auto_minmax(0,1fr)_8rem_5.5rem_7rem_9.5rem_1rem]"
      >
        <RowDot phase={phase} running={running} solved={solved} label={stateLabel} />
        <RowTitle lab={lab} emulated={emulated} setup={setup} running={running} />
        <RowColumns lab={lab} hostArch={hostArch} emulates={emulates} running={running} status={status} />

        <span className="flex items-center justify-end gap-1.5">
          <RowAction
            status={status}
            running={running}
            parked={parked}
            busy={busy}
            doing={doing}
            setup={setup}
            connectCloud={connectCloud}
            loggedIn={loggedIn}
            onLogin={onLogin}
            onStop={() => setConfirmingStop(true)}
            onResume={onResume && (() => onResume(lab))}
          />
          {/* Starting happens on the lab's own page, where you pick where to run; the row opens it. */}
        </span>

        <ChevronRight className="size-4 text-faint transition-colors group-hover:text-foreground" />
      </div>

      {confirmingStop && (
        <StopDialog
          lab={lab}
          status={status}
          onCancel={() => setConfirmingStop(false)}
          onConfirm={() => {
            setConfirmingStop(false);
            onStop(lab);
          }}
        />
      )}
    </div>
  );
});
