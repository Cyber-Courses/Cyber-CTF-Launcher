"use client";

import { memo } from "react";
import { ChevronRight } from "lucide-react";
import type { Lab } from "@/features/labs/use-labs";
import { IDLE_ROW, rowFacts, rowPhase, type RowPhase, type RowSituation } from "@/features/labs/lab-row-state";
import { structurallyEqual } from "@/lib/equal";
import { RowColumns, RowDot, RowTitle } from "@/features/labs/lab-row-parts";
import { RowAction } from "@/features/labs/lab-row-actions";
import { confirmRemoveOnRow } from "@/features/labs/confirm-remove";
import type { LabStatus } from "@/lib/tauri";
import { OPERATION_STATUS } from "@/features/labs/runs/operations";
import { useT } from "@/lib/i18n";

const PHASE_WORDS = {
  running: "labs.state.running",
  failed: "labs.state.failed",
  paused: "labs.state.paused",
  shutDown: "labs.state.shutDown",
  solved: "labs.state.solved",
  notStarted: "labs.state.notStarted",
} as const satisfies Record<Exclude<RowPhase, "busy" | "starting">, string>;

type LabRowProps = {
  lab: Lab;
  status?: LabStatus;
  /** What the list knows of it beside its status (none: nothing is happening to it). */
  situation?: RowSituation;
  loggedIn: boolean;
  /** Logged out: the start button logs in instead of being greyed out. */
  onLogin?: () => Promise<boolean>;
  hostArch: string;
  /** An emulator (QEMU) is ready here, so a VM lab built for another CPU can run, slowly. */
  emulates?: boolean;
  onOpen: (lab: Lab) => void;
  onStop: (lab: Lab) => void;
  /** Brings a paused or shut-down lab back as it was. */
  onResume?: (lab: Lab) => void;
};

/** Whether a row would render the same: every prop the same, its situation by content (a list
 *  builds a new one on every render). */
function sameRow({ situation: a, ...propsA }: LabRowProps, { situation: b, ...propsB }: LabRowProps): boolean {
  const keys = Object.keys(propsA) as (keyof typeof propsA)[];
  return keys.length === Object.keys(propsB).length && keys.every((k) => Object.is(propsA[k], propsB[k])) && structurallyEqual(a, b);
}

/**
 * One lab as a dense 3.25rem list row: a status dot, the title over mono meta, then mono columns
 * (category, level, runtime with the run-place icons), the row action (Open and Stop while it
 * runs, Resume when parked, setup or sign-in when needed) and a chevron into the detail
 * (starting happens on the lab's own page). The secondary columns fold away in a narrow
 * container. Reused on the Overview rail-list and the full Labs list. Memoized: the handlers take
 * the lab, so a list passes the same ones to every row and a poll re-renders only the rows whose
 * lab changed.
 */
export const LabRow = memo(function LabRow(props: LabRowProps) {
  const { lab, status, onStop } = props;
  const t = useT();
  return (
    <div className="@container border-t border-border first:border-t-0">
      <RowLine
        {...props}
        onAskStop={() => {
          // Stop deletes the lab's machines: asked first, as the lab's own page does, offering the
          // page's Shut down where the backend's lifecycle offers it (its status's `controls`).
          const canShutdown = !!status?.controls.includes("shutdown");
          void confirmRemoveOnRow(t, { title: lab.title, canShutdown, cloud: lab.runtime?.runtime === "CLOUD" }).then((yes) => {
            if (yes) onStop(lab);
          });
        }}
      />
    </div>
  );
}, sameRow);

/** Where a row's lab stands: its facts here, its phase, what it is doing while busy and the word
 *  its dot says. */
function useRowState({ lab, status, situation = IDLE_ROW, hostArch, emulates = false }: LabRowProps) {
  const t = useT();
  const facts = rowFacts({ runtime: lab.runtime, status, hostArch, emulates });
  const { busy, operation } = situation;
  // While busy, what is actually happening: a lab being started reports running long before
  // the launch is done, so "Stopping…" there would be wrong.
  const doing = busy ? `${(operation && OPERATION_STATUS[operation]) ?? t("labs.state.working")}…` : null;
  const phase = rowPhase({ status, ...situation });
  // "Starting…": the one word for a start, as on the lab's page and in the sidebar.
  const stateLabel = phase === "busy" ? doing : phase === "starting" ? `${OPERATION_STATUS.launch ?? t("labs.state.working")}…` : t(PHASE_WORDS[phase]);
  return { ...facts, doing, phase, stateLabel };
}

/** The row itself: the dot, the title, the columns, the action and the chevron. */
function RowLine(props: LabRowProps & { onAskStop: () => void }) {
  const t = useT();
  const { lab, status, situation = IDLE_ROW, hostArch, emulates = false, onResume } = props;
  const { emulated, running, parked, doing, phase, stateLabel } = useRowState(props);
  const { busy, setup, connectCloud, solved } = situation;
  return (
    // The title is the row's one link into the lab (a stretched button: its ::after covers the
    // row, so a click anywhere opens it); the actions are its siblings, drawn above it, so no
    // control sits inside another and Tab reaches each one.
    <div className="group relative grid h-13 grid-cols-[auto_minmax(0,1fr)_auto_1rem] items-center gap-3.5 px-4 text-[0.8125rem] transition-colors hover:bg-glass has-[[data-row-open]:focus-visible]:bg-glass @min-[44rem]:grid-cols-[auto_minmax(0,1fr)_8rem_5.5rem_7rem_9.5rem_1rem]">
      <RowDot phase={phase} running={running} solved={solved} label={stateLabel} />
      <RowTitle
        lab={lab}
        emulated={emulated}
        setup={setup}
        running={running}
        failed={phase === "failed" ? stateLabel : null}
        openLabel={t("labs.row.open", { title: lab.title })}
        onOpen={() => {
          props.onOpen(lab);
        }}
      />
      <RowColumns lab={lab} hostArch={hostArch} emulates={emulates} running={running} status={status} />

      {/* Starting happens on the lab's own page, where you pick where to run; the row opens it. */}
      <span className="relative z-[1] flex items-center justify-end gap-1.5">
        <RowAction
          url={status?.url ?? null}
          state={{ running, parked, busy, doing, setup, connectCloud }}
          loggedIn={props.loggedIn}
          onLogin={props.onLogin}
          // The player wanted this lab: once signed in, its page (where it starts) opens.
          onSignedIn={() => {
            props.onOpen(lab);
          }}
          onStop={props.onAskStop}
          onResume={
            onResume &&
            (() => {
              onResume(lab);
            })
          }
        />
      </span>

      <ChevronRight aria-hidden className="size-4 text-faint transition-colors group-hover:text-foreground" />
    </div>
  );
}
