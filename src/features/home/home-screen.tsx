"use client";

import { useState } from "react";
import { ExternalLink } from "lucide-react";
import { LabRow, emulatorReady } from "@/features/labs/lab-row";
import { catalogueRowProps, UnlistedRunningRow, type RowContext } from "@/features/labs/labs-list";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useSavedServers } from "@/features/labs/use-saved-servers";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { useMachineMetrics } from "@/features/machine/use-machine-metrics";
import { DeployPanel } from "@/features/home/deploy-panel";
import { JumpBackPanel } from "@/features/home/jump-back-panel";
import { MachineSummaryPanel } from "@/features/home/machine-summary-panel";
import { HomeHeader, LabsPreviewPanel, LowRamCallout, RunningPanel, UsageStats } from "@/features/home/home-sections";
import { dockerReadiness, labContainerCount, recentLabs, runningLabs } from "@/features/home/home-model";
import { runningUnlisted } from "@/features/labs/lab-row-state";
import { CheckFailed } from "@/features/machine/check-failed";
import { getLastRun } from "@/lib/last-run";
import { useActiveOperations, useDeployingLabs } from "@/lib/deploy-store";
import { assessRam } from "@/features/home/capacity";
import type { AuthStatus, SystemReport } from "@/lib/tauri";
import { useStableCallback } from "@/lib/use-stable-callback";
import { useT } from "@/lib/i18n";

type Tab = "labs" | "machine" | "setup" | "server" | "cloud" | "settings";

export function HomeScreen({
  report,
  checkError = null,
  onRetryCheck,
  auth,
  onNavigate,
  onLogin,
}: {
  report: SystemReport | null;
  /** The machine check failed (and none answered before it). */
  checkError?: string | null;
  onRetryCheck?: () => unknown;
  auth: AuthStatus | null;
  onNavigate: (tab: Tab, slug?: string) => void;
  /** Signs in from a lab row (signed out). */
  onLogin?: () => Promise<void>;
}) {
  const t = useT();
  const loggedIn = auth?.loggedIn ?? false;
  const { labs, error, retry, statuses, completed, refreshStatus } = useLabs(loggedIn);
  const { runs, launch, stop, resume } = useLabActions(refreshStatus);
  const ops = useActiveOperations();
  // Deploys running in the backend (started before a reload, or from the website), for the rows.
  const deploying = useDeployingLabs();
  const servers = useSavedServers();
  // A rolling window of the 3 s polls, for the stat card sparklines.
  const { metrics, history: hist, failed: metricsFailed } = useMachineMetrics(3000);
  // "Now" for the "last run" labels and the greeting, taken once per visit.
  const [now] = useState(() => Date.now());

  const dockerReady = dockerReadiness(report);
  const running = runningLabs(labs ?? [], statuses);
  // Running here but not in the catalogue (it couldn't be loaded): still listed, by id.
  const unlisted = runningUnlisted(labs, statuses);
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
  const login = useStableCallback(() => onLogin?.() ?? Promise.resolve());

  // The rows read as on the Labs list (the same row): starting, solved, a failed start, what this
  // machine needs, and Sign in when signed out (the Overview has no sign-in banner).
  const ctx: RowContext = {
    statuses,
    runs,
    completed,
    deploying,
    report,
    servers,
    loggedIn,
    onLogin: !loggedIn && onLogin ? login : undefined,
    hostArch: report?.arch ?? "",
    emulates: emulatorReady(report),
    onOpen: onOpenRow,
    onStop: onStopRow,
    onResume: onResumeRow,
  };
  // The memoized row itself, no wrapper: a metrics tick re-renders the screen, not its rows.
  const labRow = (lab: Lab) => <LabRow key={lab.id} {...catalogueRowProps(lab, ctx)} />;
  const runningCount = running.length + unlisted.length;

  return (
    <div className="space-y-5">
      <HomeHeader
        auth={auth}
        now={now}
        dockerReady={dockerReady}
        running={runningCount}
        checkFailed={!report && !!checkError}
        onBrowse={() => onNavigate("labs")}
      />
      {!report && checkError && onRetryCheck && <CheckFailed error={checkError} onRetry={onRetryCheck} />}
      <UsageStats metrics={metrics} history={hist} failed={metricsFailed} />
      {capacity && capacity.level === "low" && <LowRamCallout capacity={capacity} onNavigate={onNavigate} />}

      <div className="grid grid-cols-1 items-start gap-5 lg:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
        <RunningPanel running={runningCount} containers={labContainers}>
          {running.map(labRow)}
          {unlisted.map((id) => (
            <UnlistedRunningRow key={id} id={id} />
          ))}
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
          // top of its kept machines. A cloud lab runs on an account picked on its page (it
          // costs): launched from here it only failed, with the message in a log nobody saw.
          onStart={(lab) =>
            lab.runtime?.runtime === "CLOUD" ? openLab(lab.slug) : statuses[lab.id]?.parked ? resume(lab) : launch(lab, undefined, undefined, report)
          }
        />
      )}

      <LabsPreviewPanel labs={labs} error={error} onRetry={retry} onAll={() => onNavigate("labs")}>
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
