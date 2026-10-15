"use client";

import { ChevronRight } from "lucide-react";
import { Panel } from "@/components/ui/panel";
import { engineName } from "@/features/machine/setup-steps";
import { providerLabel } from "@/features/machine/hypervisors";
import { DetailRow } from "@/features/machine/machine-parts";
import { hypervisorDetail, versionNumber } from "@/features/machine/machine-status";
import type { ProviderStatus, SystemReport, Tool } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** Tool versions, folded away for people who want them. */
export function MachineDetails({ report, hypervisors, hasHypervisor }: { report: SystemReport; hypervisors: ProviderStatus[]; hasHypervisor: boolean }) {
  const t = useT();
  const installed = t("machine.screen.detail.installed");
  const notInstalled = t("machine.screen.detail.notInstalled");
  const ver = (tool: Tool) => (tool.installed ? (versionNumber(tool) ?? installed) : notInstalled);
  return (
    <details className="group">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 text-[0.8125rem] text-muted-foreground transition-colors hover:text-foreground">
        <ChevronRight className="size-3.5 transition-transform group-open:rotate-90" /> {t("machine.screen.details")}
      </summary>
      <Panel className="mt-2.5">
        <DetailRow
          name={t("machine.screen.detail.containerEngine")}
          value={
            report.dockerEngine
              ? engineName(report.dockerEngine)
              : report.dockerRunning
                ? t("machine.screen.detail.running")
                : t("machine.screen.detail.notRunning")
          }
          bad={!report.dockerRunning}
        />
        <DetailRow name="Docker CLI" value={ver(report.docker)} bad={!report.docker.installed} />
        <DetailRow name="Docker Compose" value={ver(report.dockerCompose)} bad={!report.dockerCompose.installed} />
        <DetailRow name="Vagrant" value={ver(report.vagrant)} bad={hasHypervisor && !report.vagrant.installed} />
        {hypervisors.map((p) => {
          const detail = hypervisorDetail(p);
          return (
            <DetailRow
              key={p.provider}
              name={providerLabel(p, report.os)}
              value={
                detail === "installedWithPlugin"
                  ? t("machine.screen.detail.installedWithPlugin", { plugin: p.plugin! })
                  : detail === "pluginMissing"
                    ? t("machine.screen.detail.pluginMissing", { plugin: p.plugin! })
                    : t(`machine.screen.detail.${detail}`)
              }
              bad={p.hypervisor === true && !p.pluginInstalled}
            />
          );
        })}
        <DetailRow name={report.pkgManager.name} value={report.pkgManager.installed ? installed : notInstalled} bad={!report.pkgManager.installed} />
      </Panel>
    </details>
  );
}
