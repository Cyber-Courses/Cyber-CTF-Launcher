"use client";

import { Button } from "@/components/ui/button";
import type { LabKind } from "@/features/machine/machine-parts";
import { WANT_FREE } from "@/features/machine/machine-status";
import { formatAgo, formatBytes } from "@/lib/format";
import type { SystemReport } from "@/lib/tauri";
import type { LastTest } from "@/lib/settings";
import { useT } from "@/lib/i18n";

/** A row's tone once its provider runs labs: the last test passed or failed. */
export const TESTED_TONE = { ready: "ok", testFailed: "fail" } as const;

/** What every provider row takes. */
export type RowProps = {
  report: SystemReport;
  last: LastTest | null;
  now: number;
  /** Free memory now (null until the first metrics read). */
  memFree: number | null;
  /** The row whose test is open inline (`kind:provider`), if any. */
  testing: string | null;
  onToggleTest: (row: string) => void;
  onTestDone: () => void;
  /** Opens the setup window at a step. */
  onFix: (step: string) => void;
  /** Reads the machine again (after Docker was pointed at another engine). */
  onRefresh: () => void | Promise<void>;
};

/** The key of a row's inline test. */
export const testKey = (kind: LabKind, provider: string) => `${kind}:${provider}`;

/** Opens or hides a lab type's self-test inline. */
function TestButton({ testing, onClick }: { testing: boolean; onClick: () => void }) {
  const t = useT();
  return (
    <Button variant="outline" size="xs" onClick={onClick}>
      {testing ? t("machine.screen.hideTest") : t("machine.screen.test")}
    </Button>
  );
}

/** A row's own Test button: opens or hides its inline test. */
export function RowTestButton({ row, id }: { row: RowProps; id: string }) {
  return (
    <TestButton
      testing={row.testing === id}
      onClick={() => {
        row.onToggleTest(id);
      }}
    />
  );
}

/** What a provider row takes for its inline test: open or not, and who hears it end. */
export const rowTest = (row: RowProps, id: string) => ({ testing: row.testing === id, onTestDone: row.onTestDone });

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

/** The last test: never run, passed, or failed (and why), and how long ago. */
export function TestLine({ test, now }: { test: LastTest | null; now: number }) {
  const t = useT();
  if (!test) return <span>{t("machine.screen.notTestedYet")}</span>;
  if (test.result === "ok") return <span>{t("machine.screen.tested", { ago: formatAgo(test.at, now) })}</span>;
  return (
    <span className="text-destructive">
      {t("machine.screen.lastTestFailed", { ago: formatAgo(test.at, now) })}
      {test.why ? ` · ${test.why}` : ""}
    </span>
  );
}

/** Makes this row's engine or hypervisor the one labs run on. */
export function UseDefaultButton({ onClick }: { onClick: () => void }) {
  const t = useT();
  return (
    <Button variant="ghost" size="xs" onClick={onClick}>
      {t("machine.screen.useDefault")}
    </Button>
  );
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
