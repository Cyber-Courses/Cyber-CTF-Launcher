"use client";

import { Button } from "@/components/ui/button";
import type { LabKind } from "@/features/machine/machine-parts";
import { WANT_FREE } from "@/features/machine/machine-status";
import { formatAgo, formatBytes } from "@/lib/format";
import type { SystemReport } from "@/lib/tauri";
import type { LastTest } from "@/lib/settings";
import { useT } from "@/lib/i18n";

/** What both lab type rows take. */
export type RowProps = {
  report: SystemReport;
  last: LastTest | null;
  now: number;
  /** Free memory now (null until the first metrics read). */
  memFree: number | null;
  testing: boolean;
  onToggleTest: () => void;
  onTestDone: () => void;
  /** Opens the setup window at a step. */
  onFix: (step: string) => void;
};

/** Opens or hides a lab type's self-test inline. */
export function TestButton({ testing, onClick }: { testing: boolean; onClick: () => void }) {
  const t = useT();
  return (
    <Button variant="outline" size="xs" onClick={onClick}>
      {testing ? t("machine.screen.hideTest") : t("machine.screen.test")}
    </Button>
  );
}

/** Opens the setup window at `step`. */
export function FixButton({ step, onFix }: { step: string; onFix: (step: string) => void }) {
  const t = useT();
  return (
    <Button
      variant="outline"
      size="xs"
      onClick={() => {
        onFix(step);
      }}
    >
      {t("machine.screen.fix")}
    </Button>
  );
}

/** The last test: never run, passed, or failed, and how long ago. */
export function TestLine({ test, now }: { test: LastTest | null; now: number }) {
  const t = useT();
  if (!test) return <span>{t("machine.screen.notTestedYet")}</span>;
  if (test.result === "ok") return <span>{t("machine.screen.tested", { ago: formatAgo(test.at, now) })}</span>;
  return <span className="text-destructive">{t("machine.screen.lastTestFailed", { ago: formatAgo(test.at, now) })}</span>;
}

/** Whether free memory (null before the first read) is short for a lab type. */
export const lowFor = (kind: LabKind, memFree: number | null) => memFree !== null && memFree < WANT_FREE[kind];

/** The warning when free memory is short for a lab type, else nothing. */
export function useFreeHint(kind: LabKind, memFree: number | null): string | undefined {
  const t = useT();
  if (memFree === null || memFree >= WANT_FREE[kind]) return undefined;
  const vars = { want: formatBytes(WANT_FREE[kind]), free: formatBytes(Math.max(0, memFree)) };
  return kind === "vm" ? t("machine.screen.freeHintVm", vars) : t("machine.screen.freeHintDocker", vars);
}
