"use client";

import { Plus, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PageHeader, StatusStrip } from "@/components/ui/page-header";
import { StatusDot } from "@/components/ui/status-pill";
import type { MachineMetrics, SystemReport } from "@/lib/tauri";
import { CalloutRow, StatCard } from "@/features/machine/machine-parts";
import { OS_NAME, isLowDisk, usage } from "@/features/machine/machine-status";
import { formatBytes, formatBytesOf, formatUptime } from "@/lib/format";
import { useOpenSetup } from "@/features/machine/use-open-setup";
import { useT } from "@/lib/i18n";

// The sections of the Machine page: its header and its live usage.

/** Title, readiness strip (setup left, OS, uptime) and "Add a provider", which opens the guided
 *  setup on its choice of provider (as the Servers page's "Add server" opens the server setup). */
export function MachineHeader({ report, m, needsSetup }: { report: SystemReport; m: MachineMetrics | null; needsSetup: number }) {
  const t = useT();
  const openSetup = useOpenSetup();
  return (
    <PageHeader
      title={t("machine.screen.title")}
      lead={
        <>
          {t("machine.screen.lead")}
          <ReadinessStrip report={report} m={m} needsSetup={needsSetup} />
        </>
      }
      actions={
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            openSetup();
          }}
        >
          <Plus className="size-3.5" /> {t("machine.screen.addProvider")}
        </Button>
      }
    />
  );
}

/** Setup left (or ready), the OS and, once read, the uptime. */
function ReadinessStrip({ report, m, needsSetup }: { report: SystemReport; m: MachineMetrics | null; needsSetup: number }) {
  const t = useT();
  return (
    <StatusStrip className="mt-2.5">
      <StatusDot tone={needsSetup ? "warn" : "ok"} />
      <b>{needsSetup ? t("machine.screen.needsSetup", { count: needsSetup }) : t("machine.screen.readyForLabs")}</b>
      <span>·</span>
      <span>
        {OS_NAME[report.os] ?? report.os} {report.arch}
      </span>
      {m && (
        <>
          <span>·</span>
          <span>{t("machine.screen.uptime", { uptime: formatUptime(m.uptimeSecs) })}</span>
        </>
      )}
    </StatusStrip>
  );
}

/** CPU, memory and disk as meters. */
export function UsageCards({ m }: { m: MachineMetrics | null }) {
  const t = useT();
  const use = m ? usage(m) : null;
  return (
    <div className="grid gap-4 sm:grid-cols-3">
      <StatCard label={t("machine.screen.cpu")} detail={m ? t("machine.screen.cores", { count: m.cores }) : ""} value={m ? m.cpu : null} meter />
      <StatCard label={t("machine.screen.memory")} detail={m ? formatBytesOf(m.memUsed, m.memTotal) : ""} value={use ? use.memPct : null} meter />
      <StatCard label={t("machine.screen.disk")} detail={m ? formatBytesOf(m.diskUsed, m.diskTotal) : ""} value={use ? use.diskPct : null} meter />
    </div>
  );
}

/** Low disk: labs and the attack box are several GB each, so say so before a download fails. */
export function LowDiskCallout({ m }: { m: MachineMetrics | null }) {
  const t = useT();
  if (!m || !isLowDisk(m)) return null;
  const free = formatBytes(usage(m).diskFree);
  return (
    <CalloutRow
      tone="warn"
      icon={<TriangleAlert className="size-4" />}
      title={t("machine.screen.lowDisk.title")}
      meta={t("machine.screen.lowDisk.meta", { free })}
    >
      {t("machine.screen.lowDisk.body", { free })}
    </CalloutRow>
  );
}
