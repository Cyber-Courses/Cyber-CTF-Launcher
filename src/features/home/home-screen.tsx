"use client";

import { useState } from "react";
import { LabRow, emulatorReady } from "@/features/labs/lab-row";
import { catalogueRowProps, UnlistedRunningRow, type RowContext } from "@/features/labs/labs-list";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useSavedServers } from "@/features/labs/use-saved-servers";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { useMachineMetrics } from "@/features/machine/use-machine-metrics";
import { DeployPanel } from "@/features/home/deploy-panel";
import { JumpBackPanel } from "@/features/home/jump-back-panel";
import { MachineSummaryPanel } from "@/features/home/machine-summary-panel";
import { HomeHeader, LowRamCallout, SignInHint, UsageStats } from "@/features/home/home-sections";
import { LabsPreviewPanel, RunningPanel } from "@/features/home/lab-panels";
import { dockerReadiness, labContainerCount, recentLabs, runningLabs } from "@/features/home/home-model";
import { runningUnlisted } from "@/features/labs/lab-row-state";
import { CheckFailed } from "@/features/machine/check-failed";
import { getLastRun } from "@/features/labs/runs/last-run";
import { useActiveOperations, useDeployingLabs } from "@/features/labs/runs/operations";
import { assessRam } from "@/features/home/capacity";
import type { ActiveOperation, AuthStatus, LabStatus, MachineMetrics, SystemReport } from "@/lib/tauri";
import { useStableCallback } from "@/lib/use-stable-callback";

type Tab = "labs" | "machine" | "setup" | "server" | "cloud" | "settings";

type HomeProps = {
  report: SystemReport | null;
  /** The machine check failed (and none answered before it). */
  checkError?: string | null;
  onRetryCheck?: () => unknown;
  auth: AuthStatus | null;
  onNavigate: (tab: Tab, slug?: string) => void;
  /** Signs in from a lab row (signed out). */
  onLogin?: () => Promise<void>;
};

/** The labs as the Overview shows them: the catalogue, what runs and deploys, what to jump back
 *  into, and the actions on them. */
function useOverviewLabs(loggedIn: boolean) {
  const { labs, error, retry, statuses, completed, refreshStatus } = useLabs(loggedIn);
  const actions = useLabActions(refreshStatus);
  const ops = useActiveOperations();
  // Deploys running in the backend (started before a reload, or from the website), for the rows.
  const deploying = useDeployingLabs();
  const servers = useSavedServers();
  const all = labs ?? [];
  const running = runningLabs(all, statuses);
  return {
    labs,
    error,
    retry,
    statuses,
    completed,
    actions,
    deploying,
    servers,
    running,
    // Running here but not in the catalogue (it couldn't be loaded): still listed, by id.
    unlisted: runningUnlisted(labs, statuses),
    labContainers: labContainerCount(running, statuses),
    activeOps: [...ops.values()],
    recent: recentLabs(all, statuses, getLastRun),
  };
}

type OverviewLabs = ReturnType<typeof useOverviewLabs>;

/** The rows read as on the Labs list (the same row): starting, solved, a failed start, what this
 *  machine needs, and Sign in when signed out (the Overview has no sign-in banner). The same
 *  handlers on every render, so the memoized rows re-render only when their lab changes. */
function useRowContext(
  o: OverviewLabs,
  { report, loggedIn, onLogin, openLab }: { report: SystemReport | null; loggedIn: boolean; onLogin?: () => Promise<void>; openLab: (slug: string) => void },
): RowContext {
  const { stop, resume } = o.actions;
  const onOpen = useStableCallback((lab: Lab) => {
    openLab(lab.slug);
  });
  const onStop = useStableCallback((lab: Lab) => void stop(lab));
  const onResume = useStableCallback((lab: Lab) => void resume(lab));
  const login = useStableCallback(() => onLogin?.() ?? Promise.resolve());
  return {
    statuses: o.statuses,
    runs: o.actions.runs,
    completed: o.completed,
    deploying: o.deploying,
    report,
    servers: o.servers,
    loggedIn,
    onLogin: !loggedIn && onLogin ? login : undefined,
    hostArch: report?.arch ?? "",
    emulates: emulatorReady(report),
    onOpen,
    onStop,
    onResume,
  };
}

/** Starts a recent lab again: a shut-down or paused lab comes back as it was (a fresh launch would
 *  start over on top of its kept machines). A cloud lab runs on an account picked on its page (it
 *  costs): launched from here it only failed, with the message in a log nobody saw. */
function startAgain(o: OverviewLabs, report: SystemReport | null, openLab: (slug: string) => void) {
  const statuses: Partial<Record<string, LabStatus>> = o.statuses;
  return (lab: Lab) => {
    if (lab.runtime?.runtime === "CLOUD") openLab(lab.slug);
    else if (statuses[lab.id]?.parked) void o.actions.resume(lab);
    else void o.actions.launch(lab, undefined, undefined, report);
  };
}

/** The second column: what is being deployed right now, else a summary of this machine. */
function SecondColumn({
  ops,
  labs,
  report,
  metrics,
  openLab,
  onMachine,
}: {
  ops: ActiveOperation[];
  labs: Lab[];
  report: SystemReport | null;
  metrics: MachineMetrics | null;
  openLab: (slug: string) => void;
  onMachine: (tab: "machine") => void;
}) {
  if (ops.length > 0) return <DeployPanel ops={ops} labs={labs} onOpen={openLab} />;
  return (
    <MachineSummaryPanel
      report={report}
      metrics={metrics}
      dockerReady={dockerReadiness(report)}
      onOpen={() => {
        onMachine("machine");
      }}
    />
  );
}

export function HomeScreen({ report, checkError = null, onRetryCheck, auth, onNavigate, onLogin }: HomeProps) {
  const loggedIn = auth?.loggedIn ?? false;
  const o = useOverviewLabs(loggedIn);
  // A rolling window of the 3 s polls, for the stat card sparklines.
  const { metrics, history, failed: metricsFailed } = useMachineMetrics(3000);
  // "Now" for the "last run" labels and the greeting, taken once per visit.
  const [now] = useState(() => Date.now());
  const dockerReady = dockerReadiness(report);
  const capacity = metrics ? assessRam(metrics.memTotal) : null;
  const openLab = (slug: string) => {
    onNavigate("labs", slug);
  };
  const ctx = useRowContext(o, { report, loggedIn, onLogin, openLab });
  // The memoized row itself, no wrapper: a metrics tick re-renders the screen, not its rows.
  const labRow = (lab: Lab) => <LabRow key={lab.id} {...catalogueRowProps(lab, ctx)} />;
  const runningCount = o.running.length + o.unlisted.length;
  const browse = () => {
    onNavigate("labs");
  };

  return (
    <div className="space-y-5">
      <HomeHeader auth={auth} now={now} dockerReady={dockerReady} running={runningCount} checkFailed={!report && !!checkError} onBrowse={browse} />
      {!report && checkError && onRetryCheck && <CheckFailed error={checkError} onRetry={onRetryCheck} />}
      <UsageStats metrics={metrics} history={history} failed={metricsFailed} />
      {capacity && capacity.level === "low" && <LowRamCallout capacity={capacity} onNavigate={onNavigate} />}

      <div className="grid grid-cols-1 items-start gap-5 lg:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
        <RunningPanel running={runningCount} containers={o.labContainers}>
          {o.running.map(labRow)}
          {o.unlisted.map((id) => (
            <UnlistedRunningRow key={id} id={id} />
          ))}
        </RunningPanel>
        <SecondColumn ops={o.activeOps} labs={o.labs ?? []} report={report} metrics={metrics} openLab={openLab} onMachine={onNavigate} />
      </div>

      {o.recent.length > 0 && (
        <JumpBackPanel
          recent={o.recent}
          now={now}
          isBusy={(lab) => !!o.actions.runs[lab.id]?.busy}
          canStart={(lab) => loggedIn && !!lab.runtime}
          onOpen={(lab) => {
            openLab(lab.slug);
          }}
          onStart={startAgain(o, report, openLab)}
        />
      )}

      <LabsPreviewPanel labs={o.labs} error={o.error} onRetry={o.retry} onAll={browse}>
        {(o.labs ?? []).slice(0, 6).map(labRow)}
      </LabsPreviewPanel>

      {!auth?.loggedIn && <SignInHint />}
    </div>
  );
}
