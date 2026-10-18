"use client";

import { StatusDot } from "@/components/ui/status-pill";
import type { SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { engineSummary } from "@/components/shell/shell-model";

/** The main column's top bar (drags the window): the breadcrumb, then the container engine and
 *  hypervisor this machine runs labs on. */
export function ShellHeader({ crumbs, report, checkError }: { crumbs: string[]; report: SystemReport | null; checkError: string | null }) {
  const t = useT();
  const summary = report ? engineSummary(report) : null;
  return (
    <div data-tauri-drag-region className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-6 text-[0.8125rem] text-faint">
      {crumbs.map((c, i) =>
        i === crumbs.length - 1 ? (
          <b key={i} className="pointer-events-none truncate font-medium text-foreground">
            {c}
          </b>
        ) : (
          <span key={i} className="pointer-events-none flex items-center gap-2">
            {c}
            <span>/</span>
          </span>
        ),
      )}
      <span className="pointer-events-none ml-auto flex items-center gap-2 font-mono text-[0.6875rem]">
        {report && summary ? (
          <>
            <StatusDot tone={report.dockerRunning ? "ok" : "warn"} />
            {summary.engine ?? t("shell.header.dockerNotRunning")}
            {summary.hypervisor && <span> · {summary.hypervisor}</span>}
          </>
        ) : checkError ? (
          <>
            <StatusDot tone="fail" /> {t("shell.header.checkFailed")}
          </>
        ) : (
          <>
            <StatusDot tone="muted" /> {t("shell.header.checking")}
          </>
        )}
      </span>
    </div>
  );
}
