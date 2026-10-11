"use client";

import { useState } from "react";
import { PageHeader } from "@/components/ui/page-header";
import { StatusDot } from "@/components/ui/status-pill";
import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { HostedSessionPanel } from "@/features/labs/hosted-session-panel";
import { LabBrief } from "@/features/labs/lab-brief";
import { HealthBanner, useLabCheck } from "@/features/labs/lab-health";
import type { RunTarget } from "@/features/labs/run-on";
import { useAttackBox } from "@/features/labs/use-attack-box";
import type { Lab } from "@/features/labs/use-labs";
import { DeploymentPanel } from "@/features/labs/lab-detail/deployment-panel";
import { DetailsPanel } from "@/features/labs/lab-detail/details-panel";
import { HeaderActions } from "@/features/labs/lab-detail/header-actions";
import { HeaderLead } from "@/features/labs/lab-detail/header-lead";
import { deployDestination, runtimeWord } from "@/features/labs/lab-detail/labels";
import { cloudView, deployPlacement, headerMode, networkView, restartTarget } from "@/features/labs/lab-detail/lab-state";
import { CloudNotePanel, CloudPanel, costEstimate, useCloudPreview } from "@/features/labs/cloud-panel";
import { NetworkPanel } from "@/features/labs/lab-detail/network-panel";
import { BackLink, ObjectivePanel } from "@/features/labs/lab-detail/page-parts";
import { ObserversPanel } from "@/features/labs/lab-detail/observers-panel";
import { RemoveDialog } from "@/features/labs/lab-detail/remove-dialog";
import { LabDetailSkeleton } from "@/features/labs/lab-detail/skeleton";
import { StartLabButton } from "@/features/labs/lab-detail/start-lab-button";
import { useEscapeBack } from "@/features/labs/lab-detail/use-escape-back";
import { useHostedRun } from "@/features/labs/lab-detail/use-hosted-run";
import { useLabPageState } from "@/features/labs/lab-detail/use-lab-page-state";
import { useLabShell } from "@/features/labs/lab-detail/use-lab-shell";
import { useLabTools } from "@/features/labs/lab-detail/use-lab-tools";
import { usePageActions } from "@/features/labs/lab-detail/use-page-actions";
import { useRunTarget } from "@/features/labs/lab-detail/use-run-target";
import { WhereItRunsPanel } from "@/features/labs/lab-detail/where-it-runs-panel";
import { type LabStatus, type Park, type Provider } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** A lab's own page: what it is, its state and the one thing to do next, then its deployment,
 *  network, brief and side rail (objective, attack box, where it runs, details). */
export function LabDetail({
  lab,
  status,
  busy,
  logs,
  times,
  loggedIn,
  onLogin,
  hostArch,
  readyVms = [],
  dockerRunning = null,
  onBack,
  onStart,
  onStop,
  onPark,
  onResume,
  onProvision,
  ready = true,
}: {
  lab: Lab;
  status?: LabStatus;
  busy: boolean;
  logs: string[];
  times: number[];
  loggedIn: boolean;
  /** Logged out: the start button logs in instead of being greyed out. */
  onLogin?: () => Promise<void>;
  hostArch: string;
  /** Local hypervisors ready on this machine (Vagrant + hypervisor), for "in a VM". */
  readyVms?: Provider[];
  /** Whether a Docker engine answers on this machine (null = unknown). */
  dockerRunning?: boolean | null;
  onBack: () => void;
  onStart: (target: RunTarget) => Promise<void> | void;
  /** Resolves to the failure when the stop failed (Reset then doesn't start the lab again). */
  onStop: () => Promise<string | null | void> | void;
  /** Pause (state saved) or shut down (powered off), machines kept for `onResume`. */
  onPark?: (mode: Park) => Promise<unknown> | void;
  onResume?: () => Promise<unknown> | void;
  /** Re-run a VM lab's provisioners on one machine (null = all), in place. */
  onProvision?: (machine: string | null) => Promise<unknown> | void;
  /** False until the first checks (sign-in, the lab's status) are in: the page then shows a
   *  placeholder instead of a Start button that flips to Running a moment later. */
  ready?: boolean;
}) {
  const t = useT();
  useEscapeBack(onBack);
  const rt = lab.runtime;
  const { acting, act, provisionTarget, setProvisionTarget } = usePageActions({ onPark, onResume, onProvision });
  const s = useLabPageState(lab.id, {
    status,
    runtime: rt,
    hostArch,
    readyVms,
    busy,
    acting,
    parkable: !!onPark,
    provisionable: !!onProvision,
    logs,
    times,
  });
  const run = useRunTarget(rt, readyVms, hostArch);
  const preview = useCloudPreview(lab.id, rt);
  const hosted = useHostedRun(lab.id);
  const [resetting, setResetting] = useState(false);
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const shell = useLabShell(lab.id, lab.title, s.remote || s.isDocker);

  // The attack box: a container on a local container lab's networks, the learner's own VM beside
  // a local VM lab; a remote container lab's runs next to it on its host.
  // Not while the lab itself is starting, resuming or stopping: a resume brings the attack VM
  // back on its own, and a second `vagrant up` in its folder at the same time would collide.
  // A failed deploy can leave the VMs up (a provisioning step broke): the lab then reads as
  // running, but its attack box must not start beside a lab that isn't ready.
  // A cloud lab has none: the player works from their own machine.
  const box = useAttackBox(lab.id, {
    running: s.running && !s.isCloud,
    holding: s.deploying,
    local: !s.remote && !s.isCloud,
    kind: s.isDocker ? "container" : "vm",
    failed: s.deployFailed,
  });
  const [check, clearCheck] = useLabCheck(lab.id, { running: s.running, downCount: s.down.length, enabled: s.isDocker });
  const tools = useLabTools(lab.id, s.running && !s.isCloud, s.isDocker);

  // Reset = stop and start again where it ran: a clean lab.
  async function reset() {
    setResetting(true);
    try {
      // A stop that failed left the lab half removed: starting over it would build on the wreck.
      const failure = await onStop();
      if (typeof failure === "string") {
        tell(t("labs.health.resetStopFailed"))(failure);
        return;
      }
      await onStart(restartTarget(status, run.hosts, run.runOn, run.localVm));
    } finally {
      setResetting(false);
      clearCheck();
    }
  }

  const startOn = (target: RunTarget) => (target.kind === "hosted" ? hosted.launch() : void onStart(target));

  if (!ready) return <LabDetailSkeleton />;

  const meta = runtimeWord(t, s);
  const mode = headerMode(s, { loggedIn, canLogin: !!onLogin, canResume: !!onResume });
  const placement = deployPlacement(s, { busy, failed: s.deployFailed });
  const network = s.isCloud ? "none" : networkView(s, { busy, acting, hasMachines: !!status && status.machines.length > 0 });
  const services = s.isCloud ? cloudView(s, { busy }) : "none";
  // The lab's own hourly estimate, for the aside.
  const cost = s.isCloud ? costEstimate(t, preview?.hourlyUsd) : null;
  const deploy = (
    <DeploymentPanel
      lines={s.shownLogs}
      times={s.shownTimes}
      busy={s.deploying}
      ready={s.running && !s.tearingDown}
      where={deployDestination(t, status, run.runOn, run.hosts, s.engine)}
      operation={s.operation ?? s.lastOperation}
      offerRerun={s.deployFailed && s.canProvision && !busy && (s.running || s.interrupted)}
      rerunning={acting === "provision"}
      onRerun={() => void act("provision")}
    />
  );

  return (
    <div className="animate-rise-in space-y-5">
      <BackLink onBack={onBack} />

      {/* Header: what the lab is, its state at a glance, and the one thing to do next. */}
      <PageHeader
        title={lab.title}
        lead={<HeaderLead lab={lab} state={s} status={status} />}
        actions={
          mode === "start" ? (
            <StartLabButton
              lab={lab}
              run={run}
              isDocker={s.isDocker}
              cloud={s.isCloud ? { preview } : null}
              loggedIn={loggedIn}
              hostedLive={hosted.live}
              busy={busy}
              dockerRunning={dockerRunning}
              onStart={startOn}
            />
          ) : (
            <HeaderActions
              mode={mode}
              state={s}
              url={status?.url}
              busy={busy}
              deploying={s.deploying}
              acting={acting}
              resetting={resetting}
              onAct={(what) => void act(what)}
              onRemove={() => setConfirmingRemove(true)}
              onStop={onStop}
              onLogin={onLogin}
            />
          )
        }
      />
      {shell.error && (
        <p className="-mt-2 flex items-center gap-2 text-[0.75rem] text-muted-foreground">
          <StatusDot tone="fail" /> {shell.error}
        </p>
      )}
      {hosted.shown && <HostedSessionPanel session={hosted.session} starting={hosted.starting} error={hosted.error} onStop={hosted.stop} />}

      {/* Health: a container went down. Say so, check it, offer a clean restart. */}
      {s.running && s.down.length > 0 && (
        <HealthBanner
          down={s.down.map((m) => m.name)}
          check={check}
          busy={busy}
          resetting={resetting}
          onReset={reset}
          onResume={!s.isDocker && !s.remote && onResume ? () => void act("resume") : undefined}
        />
      )}

      <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_19rem]">
        <div className="min-w-0 space-y-5">
          {placement === "top" && deploy}
          {services === "services" && <CloudPanel message={status?.message ?? null} outputs={status?.outputs ?? []} where={status?.host ?? meta} />}
          {(services === "cloudLeftover" || services === "cloudIdle") && <CloudNotePanel note={services} meta={meta} />}
          {network !== "none" && <NetworkPanel view={network} status={status} remote={s.remote} box={box.status} meta={rt ? meta : undefined} />}
          {s.running && !busy && tools.length > 0 && <ObserversPanel tools={tools} />}
          {placement === "bottom" && deploy}
          <LabBrief labId={lab.id} />
        </div>

        <aside className="h-fit min-w-0 space-y-5 lg:sticky lg:top-2">
          {lab.question && <ObjectivePanel question={lab.question} />}

          {/* VM labs on a server host have no attacker yet (their networks live on that host). */}
          {!s.isCloud && (s.isDocker || !s.remote) && (
            <AttackBoxPanel
              box={box}
              running={s.running && !s.deploying}
              host={s.remote ? (status?.host ?? null) : null}
              onShell={shell.open}
              shellReady={s.remote ? s.isDocker : !!box.status?.running}
              controller={!s.isDocker && status?.provider === "qemu"}
            />
          )}

          {!s.running && rt && (
            <WhereItRunsPanel runtime={rt} hostArch={hostArch} native={s.native} emulates={s.emulates} isDocker={s.isDocker} meta={meta} cost={cost} />
          )}

          {s.running && (
            <DetailsPanel
              status={status}
              engine={s.engine}
              meta={meta}
              cost={cost}
              provision={
                s.canProvision
                  ? {
                      target: provisionTarget,
                      onTarget: setProvisionTarget,
                      busy: busy || s.deploying,
                      running: acting === "provision",
                      onRun: () => void act("provision"),
                    }
                  : null
              }
            />
          )}
          {!loggedIn && <p className="px-1 text-[0.75rem] text-muted-foreground">{t("labs.detail.signInToRun")}</p>}
        </aside>
      </div>
      {confirmingRemove && (
        <RemoveDialog
          title={lab.title}
          canShutdown={s.canShutdown}
          cloud={s.isCloud}
          onCancel={() => setConfirmingRemove(false)}
          onConfirm={() => {
            setConfirmingRemove(false);
            void onStop();
          }}
        />
      )}
    </div>
  );
}
