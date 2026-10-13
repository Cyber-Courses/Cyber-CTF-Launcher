"use client";

import { useState } from "react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { machineOpenSetup, type SystemReport } from "@/lib/tauri";
import { getLastTest, getVmProvider, type LastTest } from "@/lib/settings";
import { LabKind } from "@/features/machine/machine-parts";
import { DockerLabRow, VmLabRow } from "@/features/machine/lab-type-rows";
import { MachineDetails } from "@/features/machine/machine-details";
import { LowDiskCallout, MachineHeader, UsageCards, type MachineView } from "@/features/machine/machine-sections";
import { dockerState, setupCount, usage, vmSetup } from "@/features/machine/machine-status";
import { SetupChecklist } from "@/features/machine/setup-checklist";
import { DownloadsPanel } from "@/features/machine/downloads-panel";
import { RunningNowPanel } from "@/features/machine/running-now-panel";
import { useMachineMetrics } from "@/features/machine/use-machine-metrics";
import { useRefreshOnFocus } from "@/features/machine/use-refresh-on-focus";
import { useNow } from "@/lib/use-now";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

const lastTests = (): Record<LabKind, LastTest | null> => ({ docker: getLastTest("docker"), vm: getLastTest("vm") });

/** Last self-test per lab type (kept locally), which test is open inline, and the "now" the
 *  rows and the running list read. */
function useLabTests() {
  const [last, setLast] = useState(lastTests);
  const [testing, setTesting] = useState<LabKind | null>(null);
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
  const t = useT();
  useRefreshOnFocus(onRefresh);

  // Live usage (the stat cards show meters; the Overview keeps the sparkline history).
  const { metrics: m } = useMachineMetrics(2500, { serial: true });

  // Health (usage, engines, what runs) or Setup (the guided steps, done or not).
  const [view, setView] = useState<MachineView>("health");

  const { last, testing, setTesting, now, refreshLast } = useLabTests();

  const docker = dockerState(report, last.docker);
  const vm = vmSetup(report, getVmProvider(), last.vm);
  const needsSetup = setupCount(docker, vm.state);
  const use = m ? usage(m) : null;

  const fix = (step: string) => machineOpenSetup(step).catch(tell(t("machine.errors.openSetup")));
  const rowProps = (kind: LabKind) => ({
    report,
    last: last[kind],
    now,
    memFree: use?.memFree ?? null,
    testing: testing === kind,
    onToggleTest: () => setTesting(testing === kind ? null : kind),
    onTestDone: refreshLast,
    onFix: fix,
  });

  return (
    <div className="space-y-5">
      <MachineHeader report={report} m={m} needsSetup={needsSetup} view={view} onView={setView} />

      {view === "setup" ? (
        <SetupChecklist report={report} last={last} onFix={fix} />
      ) : (
        <>
          <UsageCards m={m} />
          <LowDiskCallout m={m} />

          {/* What can run */}
          <Panel>
            <PanelHeader title={t("machine.screen.engines")} meta={t("machine.screen.enginesMeta")} />
            <DockerLabRow {...rowProps("docker")} />
            <VmLabRow {...rowProps("vm")} vm={vm} onUseServer={() => onNavigate("server")} />
          </Panel>

          <div className="grid items-start gap-5 lg:grid-cols-2">
            <RunningNowPanel refreshKey={now} />
            <DownloadsPanel />
          </div>

          <MachineDetails report={report} hypervisors={vm.hypervisors} hasHypervisor={vm.hasHypervisor} />
        </>
      )}
    </div>
  );
}
