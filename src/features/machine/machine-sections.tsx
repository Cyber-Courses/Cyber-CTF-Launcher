"use client";

import { TriangleAlert, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PageHeader, StatusStrip } from "@/components/ui/page-header";
import { Segmented } from "@/components/ui/segmented";
import { StatusDot } from "@/components/ui/status-pill";
import { machineOpenSetup, type MachineMetrics, type SystemReport } from "@/lib/tauri";
import { CalloutRow, StatCard } from "@/features/machine/machine-parts";
import { OS_NAME, isLowDisk, usage } from "@/features/machine/machine-status";
import { useMachineFormat } from "@/features/machine/use-machine-format";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

// The sections of the Machine page: its header and the live usage of the Health view.

export type MachineView = "health" | "setup";

/** Title, readiness strip (setup left, OS, uptime), the Set up button and the Health/Setup switch. */
export function MachineHeader({
  report,
  m,
  needsSetup,
  view,
  onView,
}: {
  report: SystemReport;
  m: MachineMetrics | null;
  needsSetup: number;
  view: MachineView;
  onView: (view: MachineView) => void;
}) {
  const t = useT();
  const fmt = useMachineFormat();
  return (
    <PageHeader
      title={t("machine.screen.title")}
      lead={
        <>
          {t("machine.screen.lead")}
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
                <span>{t("machine.screen.uptime", { uptime: fmt.uptime(m.uptimeSecs) })}</span>
              </>
            )}
          </StatusStrip>
        </>
      }
      actions={
        <>
          {needsSetup > 0 && (
            <Button size="sm" onClick={() => machineOpenSetup().catch(tell(t("machine.errors.openSetup")))}>
              <Wrench className="size-3.5" /> {t("machine.screen.setUp")}
            </Button>
          )}
          <Segmented
            label={t("machine.screen.viewLabel")}
            value={view}
            onChange={onView}
            options={[
              { value: "health", label: t("machine.screen.viewHealth") },
              { value: "setup", label: t("machine.screen.viewSetup") },
            ]}
          />
        </>
      }
    />
  );
}

/** CPU, memory and disk as meters. */
export function UsageCards({ m }: { m: MachineMetrics | null }) {
  const t = useT();
  const fmt = useMachineFormat();
  const use = m ? usage(m) : null;
  return (
    <div className="grid gap-4 sm:grid-cols-3">
      <StatCard label={t("machine.screen.cpu")} detail={m ? t("machine.screen.cores", { count: m.cores }) : ""} value={m ? m.cpu : null} meter />
      <StatCard
        label={t("machine.screen.memory")}
        detail={m ? t("machine.screen.usedOf", { used: fmt.bytes(m.memUsed), total: fmt.bytes(m.memTotal) }) : ""}
        value={use ? use.memPct : null}
        meter
      />
      <StatCard
        label={t("machine.screen.disk")}
        detail={m ? t("machine.screen.usedOf", { used: fmt.bytes(m.diskUsed), total: fmt.bytes(m.diskTotal) }) : ""}
        value={use ? use.diskPct : null}
        meter
      />
    </div>
  );
}

/** Low disk: labs and the attack box are several GB each, so say so before a download fails. */
export function LowDiskCallout({ m }: { m: MachineMetrics | null }) {
  const t = useT();
  const fmt = useMachineFormat();
  if (!m || !isLowDisk(m)) return null;
  const free = fmt.bytes(usage(m).diskFree);
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
