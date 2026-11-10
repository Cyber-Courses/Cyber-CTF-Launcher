"use client";

import { useState } from "react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import type { MachineMetrics, Provider, SystemReport } from "@/lib/tauri";
import { getLastTest, type LastTest } from "@/lib/settings";
import { useVmProvider } from "@/lib/use-vm-provider";
import type { LabKind } from "@/features/machine/machine-parts";
import { EngineRows } from "@/features/machine/engine-rows";
import { HypervisorRows } from "@/features/machine/hypervisor-rows";
import { MachineDetails } from "@/features/machine/machine-details";
import { LowDiskCallout, MachineHeader, UsageCards } from "@/features/machine/machine-sections";
import { dockerState, setupCount, usage, vmSetup } from "@/features/machine/machine-status";
import { DownloadsPanel } from "@/features/machine/downloads-panel";
import { RunningNowPanel } from "@/features/machine/running-now-panel";
import { useMachineMetrics } from "@/features/machine/use-machine-metrics";
import { useRefreshOnFocus } from "@/features/machine/use-refresh-on-focus";
import { useNow } from "@/lib/use-now";
import { useOpenSetup } from "@/features/machine/use-open-setup";
import { useT } from "@/lib/i18n";

const lastTests = (): Record<LabKind, LastTest | null> => ({ docker: getLastTest("docker"), vm: getLastTest("vm") });

/** Last self-test per lab type (kept locally; each row reads its provider's own), which row's
 *  test is open inline, and the "now" the rows and the running list read. */
function useLabTests() {
  const [last, setLast] = useState(lastTests);
  const [testing, setTesting] = useState<string | null>(null);
  const [now, touchNow] = useNow(30_000);
  const refreshLast = () => {
    setLast(lastTests());
    // Bumping "now" also refreshes the running list (a test may have started or stopped things).
    touchNow();
  };
  return { last, testing, setTesting, now, refreshLast };
}

export function MachineScreen({
  report,
  onRefresh,
  onNavigate,
}: {
  report: SystemReport;
  onRefresh: () => void | Promise<void>;
  onNavigate: (tab: "cloud" | "server") => void;
}) {
  useRefreshOnFocus(onRefresh);
  // Live usage (the stat cards show meters; the Overview keeps the sparkline history).
  const { metrics: m } = useMachineMetrics(2500, { serial: true });
  const tests = useLabTests();
  // The hypervisor chosen in Settings (a row's "Use by default" changes it): the page follows.
  const preferred = useVmProvider();
  const vm = vmSetup(report, preferred, tests.last.vm);
  const needsSetup = setupCount(dockerState(report, tests.last.docker), vm.state);
  // A row's Fix or Test opens the guided setup at that step (the header's button, at its choice).
  const fix = useOpenSetup();

  return (
    <div className="space-y-5">
      <MachineHeader report={report} m={m} needsSetup={needsSetup} />
      <MachineBody report={report} m={m} vm={vm} tests={tests} onFix={fix} onRefresh={onRefresh} onNavigate={onNavigate} />
    </div>
  );
}

/** Usage, the providers this machine has, what runs now, the downloads, and the details. */
function MachineBody({
  report,
  m,
  vm,
  tests,
  onFix,
  onRefresh,
  onNavigate,
}: {
  report: SystemReport;
  m: MachineMetrics | null;
  vm: ReturnType<typeof vmSetup>;
  tests: ReturnType<typeof useLabTests>;
  onFix: (step: string, provider?: Provider) => void;
  onRefresh: () => void | Promise<void>;
  onNavigate: (tab: "cloud" | "server") => void;
}) {
  const t = useT();
  const { last, testing, setTesting, now, refreshLast } = tests;
  const rowProps = (kind: LabKind) => ({
    report,
    last: last[kind],
    now,
    memFree: m ? usage(m).memFree : null,
    testing,
    onToggleTest: (row: string) => {
      setTesting(testing === row ? null : row);
    },
    onTestDone: refreshLast,
    onFix,
    onRefresh,
  });
  return (
    <>
      <UsageCards m={m} />
      <LowDiskCallout m={m} />
      {/* The providers: every container engine and hypervisor installed here, nothing for the rest. */}
      <Panel>
        <PanelHeader title={t("machine.screen.providers")} meta={t("machine.screen.providersMeta")} />
        {report.dockerEngines.length === 0 && !vm.hasHypervisor && (
          <p className="px-4 py-3.5 text-[0.8125rem] text-muted-foreground">{t("machine.screen.providersEmpty")}</p>
        )}
        <EngineRows {...rowProps("docker")} />
        <HypervisorRows
          {...rowProps("vm")}
          vm={vm}
          onUseServer={() => {
            onNavigate("server");
          }}
        />
      </Panel>
      <div className="grid items-start gap-5 lg:grid-cols-2">
        <RunningNowPanel refreshKey={now} />
        <DownloadsPanel />
      </div>
      <MachineDetails report={report} hypervisors={vm.hypervisors} hasHypervisor={vm.hasHypervisor} />
    </>
  );
}
