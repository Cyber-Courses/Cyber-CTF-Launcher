"use client";

import { ArrowRight } from "lucide-react";
import { KeyValue, Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { StatusPill } from "@/components/ui/status-pill";
import type { MachineMetrics, SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The Overview's second column when nothing deploys: the engine, containers and cores. */
export function MachineSummaryPanel({
  report,
  metrics,
  dockerReady,
  onOpen,
}: {
  report: SystemReport | null;
  metrics: MachineMetrics | null;
  dockerReady: boolean | null;
  onOpen: () => void;
}) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader
        title={t("home.machine.title")}
        action={
          <Button variant="ghost" size="xs" onClick={onOpen}>
            {t("home.machine.open")} <ArrowRight />
          </Button>
        }
      />
      <KeyValue k={t("home.machine.dockerEngine")}>
        {report ? (
          <StatusPill tone={report.dockerRunning ? "ok" : "warn"}>{report.dockerRunning ? t("home.machine.running") : t("home.machine.stopped")}</StatusPill>
        ) : (
          "…"
        )}
      </KeyValue>
      <KeyValue k={t("home.machine.containers")}>{metrics ? String(metrics.containers) : dockerReady ? "0" : t("home.machine.none")}</KeyValue>
      <KeyValue k={t("home.machine.cores")}>{metrics ? String(metrics.cores) : "…"}</KeyValue>
    </Panel>
  );
}
