"use client";

import { difficultyLabel, type Lab } from "@/features/labs/use-labs";
import { runtimeLabel } from "@/features/labs/lab-runtime";
import type { RowPhase } from "@/features/labs/lab-row-state";
import { RunPlaceIcons } from "@/features/labs/run-place-icons";
import type { LabStatus } from "@/lib/tauri";
import { LevelBadge } from "@/components/ui/badge";
import { StatusDot } from "@/components/ui/status-pill";
import { useT } from "@/lib/i18n";

// The cells of a lab's list row (see LabRow): its dot, title and columns (its action and the
// stop dialog are in lab-row-actions).

/** The status dot: busy or starting (pulsing), running, a failed start, solved (jewel) or idle,
 *  and its word. */
export function RowDot({ phase, running, solved, label }: { phase: RowPhase; running: boolean; solved: boolean; label: string | null }) {
  return (
    <span title={label ?? undefined} className="relative z-[1] flex w-2 justify-center">
      {phase === "busy" || phase === "starting" ? (
        <StatusDot tone="warn" pulse />
      ) : running ? (
        <StatusDot tone="ok" />
      ) : phase === "failed" ? (
        <StatusDot tone="fail" />
      ) : solved ? (
        <span aria-hidden className="dot dot-jewel" />
      ) : (
        <StatusDot tone="muted" />
      )}
      <span className="sr-only">{label}</span>
    </span>
  );
}

/** The title over mono meta: slug, emulated, then what setup it needs or its description. The
 *  title is the button that opens the lab, stretched over the whole row by its ::after (the row is
 *  the positioned box; nothing between them may be), with the row's focus ring. */
export function RowTitle({
  lab,
  emulated,
  setup,
  running,
  failed = null,
  openLabel,
  onOpen,
}: {
  lab: Lab;
  emulated: boolean;
  setup: string | null;
  running: boolean;
  /** Its last start failed: the words, in place of the description. */
  failed?: string | null;
  /** "Open <title>": the button's name. */
  openLabel: string;
  onOpen: () => void;
}) {
  const t = useT();
  return (
    <span className="min-w-0">
      <button
        type="button"
        data-row-open
        aria-label={openLabel}
        onClick={onOpen}
        className="block w-full cursor-pointer truncate text-left font-medium text-foreground outline-none after:absolute after:inset-0 after:content-[''] focus-visible:after:ring-2 focus-visible:after:ring-ring focus-visible:after:ring-inset"
      >
        {lab.title}
      </button>
      <span className="block truncate font-mono text-[0.6875rem] text-faint">
        {lab.slug}
        {emulated && (
          <>
            {" · "}
            <span className="relative z-[1] text-warning" title={t("labs.row.emulatedHint")}>
              {t("labs.row.emulated")}
            </span>
          </>
        )}
        {failed ? (
          <>
            {" · "}
            <span className="text-destructive">{failed}</span>
          </>
        ) : setup && !running ? (
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
