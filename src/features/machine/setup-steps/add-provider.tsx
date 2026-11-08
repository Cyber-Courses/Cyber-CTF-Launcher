"use client";

import { useState } from "react";
import { Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { usableHypervisors } from "@/features/machine/hypervisors";
import { AddEngineRow } from "@/features/machine/setup-steps/add-engine-row";
import { AddHypervisorRow } from "@/features/machine/setup-steps/add-hypervisor-row";
import { enginesFor, type Engine } from "@/features/machine/setup-steps/engines";
import { useMachineSetup } from "@/features/machine/setup-steps/use-machine-setup";
import type { DockerEngine, Provider, SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

const OS_NAMES: Partial<Record<string, string>> = { windows: "Windows", macos: "macOS" };

/** What this machine could run but doesn't have yet, as the picker opened: the container engines
 *  of this OS the machine check didn't find, and the hypervisors it can use that aren't installed.
 *  Read once, so a provider installed from here stays listed with its test and "Use by default". */
function candidates(report: SystemReport): { engines: DockerEngine[]; hypervisors: Provider[] } {
  return {
    engines: enginesFor(report.os)
      .filter((e) => !report.dockerEngines.some((found) => found.engine === e.id))
      .map((e) => e.id),
    hypervisors: usableHypervisors(report)
      .filter((p) => p.hypervisor !== true)
      .map((p) => p.provider),
  };
}

/** "Add a provider": the container engines and hypervisors this machine can run but doesn't
 *  have, each with its install, its log, then its own test and "Use by default". Nothing left
 *  to install: says so. The guided lab-type setup is the first run's, not this button's. */
export function AddProvider({ report, onRefresh, onClose }: { report: SystemReport; onRefresh: () => void; onClose: () => void }) {
  const t = useT();
  const setup = useMachineSetup(report, onRefresh);
  const [list] = useState(() => candidates(report));
  const os = OS_NAMES[report.os] ?? "Linux";
  const engines = list.engines.map((id) => enginesFor(report.os).find((e) => e.id === id)).filter((e): e is Engine => !!e);
  const hypervisors = list.hypervisors.map((id) => usableHypervisors(report).find((p) => p.provider === id)).filter((p) => !!p);
  const nothing = engines.length === 0 && hypervisors.length === 0;
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <main className="mx-auto flex w-full max-w-2xl flex-col gap-5 px-6 pt-2 pb-6">
        <PageHeader title={<>{t.rich("machine.add.title", { em: (s) => <em>{s}</em> })}</>} lead={t("machine.add.lead", { os })} />
        {nothing && (
          <Panel className="flex items-center gap-3 px-4 py-4">
            <Check className="size-4 shrink-0 text-success" />
            <p className="text-[0.8125rem] text-foreground">{t("machine.add.nothing", { os })}</p>
          </Panel>
        )}
        {engines.length > 0 && (
          <Panel>
            <PanelHeader title={t("machine.add.engines")} meta={t("machine.add.enginesMeta")} />
            {engines.map((e) => (
              <AddEngineRow key={e.id} engine={e} report={report} setup={setup} />
            ))}
          </Panel>
        )}
        {hypervisors.length > 0 && (
          <Panel>
            <PanelHeader title={t("machine.add.hypervisors")} meta={t("machine.add.hypervisorsMeta")} />
            {hypervisors.map((p) => (
              <AddHypervisorRow key={p.provider} p={p} report={report} setup={setup} />
            ))}
          </Panel>
        )}
        <div className="flex justify-end">
          <Button variant="primary" size="sm" onClick={onClose} disabled={setup.busy}>
            {t("machine.setup.done")}
          </Button>
        </div>
      </main>
    </div>
  );
}
