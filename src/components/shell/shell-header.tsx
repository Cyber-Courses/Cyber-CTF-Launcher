"use client";

import { StatusDot } from "@/components/ui/status-pill";
import type { SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { engineSummary } from "@/components/shell/shell-model";
import { useVmProvider } from "@/lib/use-vm-provider";
import { WindowControls } from "@/components/window-title-bar";

/** The main column's top bar, the window's title bar on every system (it drags the window): the
 *  breadcrumb, then the container engine and hypervisor this machine runs labs on, then on Linux
 *  and Windows the window's buttons in its corner. */
export function ShellHeader({ crumbs, report, checkError }: { crumbs: string[]; report: SystemReport | null; checkError: string | null }) {
  const t = useT();
  const preferred = useVmProvider();
  const summary = report ? engineSummary(report, preferred) : null;
  return (
    <div data-tauri-drag-region className="relative flex h-12 shrink-0 select-none items-center gap-2 border-b border-border px-6 text-[0.8125rem] text-faint">
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
      <WindowControls />
    </div>
  );
}
