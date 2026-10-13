"use client";

import { useState } from "react";
import { ExternalLink } from "lucide-react";
import { LabRow, emulatorReady } from "@/features/labs/lab-row";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { useMachineMetrics } from "@/features/machine/use-machine-metrics";
import { DeployPanel } from "@/features/home/deploy-panel";
import { JumpBackPanel } from "@/features/home/jump-back-panel";
import { MachineSummaryPanel } from "@/features/home/machine-summary-panel";
import { HomeHeader, LabsPreviewPanel, LowRamCallout, RunningPanel, UsageStats } from "@/features/home/home-sections";
import { dockerReadiness, labContainerCount, recentLabs, runningLabs } from "@/features/home/home-model";
import { getLastRun } from "@/lib/last-run";
import { useActiveOperations } from "@/lib/deploy-store";
import { assessRam } from "@/features/home/capacity";
import type { AuthStatus, SystemReport } from "@/lib/tauri";
import { useStableCallback } from "@/lib/use-stable-callback";
import { useT } from "@/lib/i18n";

type Tab = "labs" | "machine" | "setup" | "server" | "cloud" | "settings";

export function HomeScreen({
  report,
  auth,
  onNavigate,
}: {
  report: SystemReport | null;
  auth: AuthStatus | null;
  onNavigate: (tab: Tab, slug?: string) => void;
}) {
  const t = useT();
  const loggedIn = auth?.loggedIn ?? false;
  const { labs, statuses, refreshStatus } = useLabs(loggedIn);
  const { runs, launch, stop, resume } = useLabActions(refreshStatus);
  const ops = useActiveOperations();
  // A rolling window of the 3 s polls, for the stat card sparklines.
  const { metrics, history: hist } = useMachineMetrics(3000);
  // "Now" for the "last run" labels and the greeting, taken once per visit.
  const [now] = useState(() => Date.now());

  const dockerReady = dockerReadiness(report);
  const running = runningLabs(labs ?? [], statuses);
  const labContainers = labContainerCount(running, statuses);
  const preview = (labs ?? []).slice(0, 6);
  const activeOps = [...ops.values()];
  const recent = recentLabs(labs ?? [], statuses, getLastRun);

  const capacity = metrics ? assessRam(metrics.memTotal) : null;
  const openLab = (slug: string) => onNavigate("labs", slug);
  // The same handlers on every render, so the memoized rows re-render only when their lab changes.
  const onOpenRow = useStableCallback((lab: Lab) => openLab(lab.slug));
  const onStopRow = useStableCallback((lab: Lab) => void stop(lab));
  const onResumeRow = useStableCallback((lab: Lab) => void resume(lab));

  const labRow = (lab: Lab) => (
    <LabRow
      key={lab.id}
      lab={lab}
      status={statuses[lab.id]}
      busy={!!runs[lab.id]?.busy}
      operation={runs[lab.id]?.op}
      loggedIn={loggedIn}
      hostArch={report?.arch ?? ""}
      emulates={emulatorReady(report)}
      onOpen={onOpenRow}
      onStop={onStopRow}
      onResume={onResumeRow}
    />
  );

  return (
    <div className="space-y-5">
      <HomeHeader auth={auth} now={now} dockerReady={dockerReady} running={running.length} onBrowse={() => onNavigate("labs")} />
      <UsageStats metrics={metrics} history={hist} />
      {capacity && capacity.level === "low" && <LowRamCallout capacity={capacity} onNavigate={onNavigate} />}

      <div className="grid grid-cols-1 items-start gap-5 lg:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
        <RunningPanel running={running.length} containers={labContainers}>
          {running.map(labRow)}
        </RunningPanel>

        {activeOps.length > 0 ? (
          <DeployPanel ops={activeOps} labs={labs ?? []} onOpen={openLab} />
        ) : (
          <MachineSummaryPanel report={report} metrics={metrics} dockerReady={dockerReady} onOpen={() => onNavigate("machine")} />
        )}
      </div>

      {recent.length > 0 && (
        <JumpBackPanel
          recent={recent}
          now={now}
          isBusy={(lab) => !!runs[lab.id]?.busy}
          canStart={(lab) => loggedIn && !!lab.runtime}
          onOpen={(lab) => openLab(lab.slug)}
          // A shut-down or paused lab comes back as it was; a fresh launch would start over on
          // top of its kept machines.
          onStart={(lab) => (statuses[lab.id]?.parked ? resume(lab) : launch(lab, undefined, undefined, report))}
        />
      )}

      <LabsPreviewPanel labs={labs} onAll={() => onNavigate("labs")}>
        {preview.map(labRow)}
      </LabsPreviewPanel>

      {!auth?.loggedIn && (
        <p className="flex items-center gap-1.5 text-[0.75rem] text-muted-foreground">
          <ExternalLink className="size-3.5" /> {t("home.signInHint")}
        </p>
      )}
    </div>
  );
}
