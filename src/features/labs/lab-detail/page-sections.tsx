"use client";

import { useState, type ReactNode } from "react";
import { Shuffle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { HostedSessionPanel } from "@/features/labs/hosted-session-panel";
import { HealthBanner } from "@/features/labs/lab-health";
import { CloudNotePanel, CloudPanel } from "@/features/labs/cloud-panel";
import { DeploymentPanel } from "@/features/labs/lab-detail/deployment-panel";
import { DetailsPanel } from "@/features/labs/lab-detail/details-panel";
import { HeaderActions } from "@/features/labs/lab-detail/header-actions";
import { deployDestination } from "@/features/labs/lab-detail/labels";
import { cloudView, networkView, removing, type HeaderMode } from "@/features/labs/lab-detail/lab-state";
import { FailureNotice, ReapFailedNotice } from "@/features/labs/lab-detail/failure-notice";
import { NetworkPanel } from "@/features/labs/lab-detail/network-panel";
import { ObjectivePanel } from "@/features/labs/lab-detail/page-parts";
import { ObserversPanel } from "@/features/labs/lab-detail/observers-panel";
import { StartLabButton } from "@/features/labs/lab-detail/start-lab-button";
import type { LabDetailProps, LabPage } from "@/features/labs/lab-detail/use-lab-detail";
import { WhereItRunsPanel } from "@/features/labs/lab-detail/where-it-runs-panel";
import { ObjectivesLoading, ObjectivesPanel } from "@/features/labs/objectives/objectives-panel";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

// The sections of a lab's page: the header's actions, the notices under it, the deployment, the
// main column's panels and the side rail.

/** The header's one thing to do next: Start (asking where first), or what the lab's state allows. */
export function LabHeaderActions({ mode, page, props: p, onRemove }: { mode: HeaderMode; page: LabPage; props: LabDetailProps; onRemove: () => void }) {
  const t = useT();
  const { s, acting, act } = page;
  if (mode === "start" || mode === "failed")
    return (
      <>
        {/* A port taken here: the same start on random free ports is the fix, one click away. */}
        {mode === "failed" && s.isDocker && s.failureKind === "ports" && (
          <Button variant="outline" onClick={() => page.startOn({ kind: "local", ports: "random" })} disabled={p.busy || !p.loggedIn}>
            <Shuffle className="size-3.5" /> {t("labs.detail.retryRandomPorts")}
          </Button>
        )}
        <StartLabButton
          retry={mode === "failed"}
          lab={p.lab}
          run={page.run}
          isDocker={s.isDocker}
          cloud={s.isCloud ? { preview: page.preview } : null}
          loggedIn={p.loggedIn}
          // Also while the hosted launch request is on its way: a second click sent it twice.
          hostedLive={page.hosted.live || page.hosted.starting}
          busy={p.busy}
          dockerRunning={p.dockerRunning ?? null}
          onStart={page.startOn}
        />
      </>
    );
  return (
    <HeaderActions
      mode={mode}
      state={s}
      url={p.status?.url}
      busy={p.busy}
      deploying={s.deploying}
      acting={acting}
      resetting={page.resetting}
      onAct={(what) => void act(what)}
      onRemove={onRemove}
      onStop={p.onStop}
      onLogin={p.onLogin}
      startedAt={s.startedAt}
    />
  );
}

/** Under the header: the last operation's failure, a shell that failed to open, the hosted
 *  session, and the health banner (a container went down: say so, check it, offer a clean
 *  restart). */
export function LabNotices({ page, props: p }: { page: LabPage; props: LabDetailProps }) {
  const { s, shell, hosted, act } = page;
  // Dismissed for this failure only: the next one shows again.
  const failure = s.failure && s.failedOp ? `${s.failedOp}|${s.startedAt ?? ""}|${s.failure}` : null;
  const [dismissed, setDismissed] = useState<string | null>(null);
  return (
    <>
      {failure && failure !== dismissed && (
        <FailureNotice
          title={p.lab.title}
          op={s.failedOp!}
          message={s.failure!}
          kind={s.failureKind}
          stillUp={s.failedOp === "stop" && s.running}
          onDismiss={() => setDismissed(failure)}
        />
      )}
      {p.status?.reapError && <ReapFailedNotice title={p.lab.title} error={p.status.reapError} />}
      {shell.error && (
        <p className="-mt-2 flex items-center gap-2 text-[0.75rem] text-muted-foreground">
          <StatusDot tone="fail" /> {shell.error}
        </p>
      )}
      {hosted.shown && <HostedSessionPanel session={hosted.session} starting={hosted.starting} error={hosted.error} onStop={hosted.stop} />}
      {s.running && s.down.length > 0 && (
        <HealthBanner
          down={s.down.map((m) => m.name)}
          check={page.check}
          busy={p.busy}
          resetting={page.resetting}
          onReset={page.reset}
          onResume={!s.isDocker && !s.remote && p.onResume ? () => void act("resume") : undefined}
        />
      )}
    </>
  );
}

/** The deployment's steps, and the way to run a broken VM setup again. */
export function LabDeployment({ page, props: p }: { page: LabPage; props: LabDetailProps }) {
  const t = useT();
  const { s, run, acting, act } = page;
  return (
    <DeploymentPanel
      lines={s.shownLogs}
      times={s.shownTimes}
      busy={s.deploying}
      ready={s.running && !s.tearingDown}
      where={deployDestination(t, p.status, run.runOn, run.hosts, s.engine)}
      operation={s.operation ?? s.lastOperation}
      startedAt={s.startedAt}
      liveStep={s.liveStep}
      offerRerun={s.deployFailed && s.canProvision && !p.busy && (s.running || s.interrupted)}
      rerunning={acting === "provision"}
      onRerun={() => void act("provision")}
    />
  );
}

/** What the network panel shows (see networkView); a cloud lab has none. */
export function labNetworkView(page: LabPage, p: LabDetailProps) {
  const { status, busy } = p;
  return page.s.isCloud ? "none" : networkView(page.s, { busy, acting: page.acting, hasMachines: !!status && status.machines.length > 0 });
}

/** The network panel: the diagram, or the note in its place. */
export function LabNetwork({ page, props: p, meta }: { page: LabPage; props: LabDetailProps; meta: string }) {
  const view = labNetworkView(page, p);
  if (view === "none") return null;
  return (
    <Dimmed on={removing(page.s)}>
      <NetworkPanel view={view} status={p.status} remote={page.s.remote} box={page.box.status} meta={p.lab.runtime ? meta : undefined} />
    </Dimmed>
  );
}

/** A panel kept in place while the lab is being removed: dimmed and out of reach (inert), so
 *  the page doesn't jump, and back as it was should the removal fail. */
function Dimmed({ on, children }: { on: boolean; children: ReactNode }) {
  return (
    <div className={cn("transition-opacity", on && "pointer-events-none opacity-50")} inert={on || undefined} aria-busy={on || undefined}>
      {children}
    </div>
  );
}

/** The main column's panels, after the deployment on top: the cloud services or note, the
 *  network (unless the page draws it above, full width), the observers. */
export function LabPanels({ page, props: p, meta, network = true }: { page: LabPage; props: LabDetailProps; meta: string; network?: boolean }) {
  const { s } = page;
  const { status, busy } = p;
  const services = s.isCloud ? cloudView(s, { busy }) : "none";
  return (
    <>
      {services === "services" && <CloudPanel message={status?.message ?? null} outputs={status?.outputs ?? []} where={status?.host ?? meta} />}
      {(services === "cloudLeftover" || services === "cloudIdle") && <CloudNotePanel note={services} meta={meta} />}
      {network && <LabNetwork page={page} props={p} meta={meta} />}
      {s.running && (!busy || removing(s)) && page.tools.length > 0 && (
        <Dimmed on={removing(s)}>
          <ObserversPanel tools={page.tools} />
        </Dimmed>
      )}
    </>
  );
}

/** The lab's challenges once read (the lab's question while they load). */
export function LabObjectives({ page, props: p }: { page: LabPage; props: LabDetailProps }) {
  const { objectives } = page;
  const { lab } = p;
  if (objectives.state.kind === "loading") return lab.question ? <ObjectivesLoading question={lab.question} /> : null;
  if (objectives.state.kind !== "ready") return null;
  return (
    <ObjectivesPanel
      // Mounted again once the player's progress is in, so the rows it opens at first skip solved ones.
      key={`${lab.id}:${objectives.state.progressRead}`}
      challenges={objectives.state.challenges}
      progress={objectives.state.progress}
      signedIn={objectives.state.signedIn}
      actions={objectives}
    />
  );
}

/** The side rail: the plain question, the attack box, where it runs (stopped) or its details
 *  (running), and the sign-in note. */
export function LabSideRail({ page, props: p, meta, cost }: { page: LabPage; props: LabDetailProps; meta: string; cost: string | null }) {
  const t = useT();
  const { s, box, shell, acting, act } = page;
  const { lab, status, busy } = p;
  const rt = lab.runtime;
  return (
    <aside className="h-fit min-w-0 space-y-5 lg:sticky lg:top-2">
      {/* A backend without the objectives API: the lab's question, as before. */}
      {page.objectives.state.kind === "unavailable" && lab.question && <ObjectivePanel question={lab.question} />}

      {/* VM labs on a server host have no attacker yet (their networks live on that host). */}
      {!s.isCloud && (s.isDocker || !s.remote) && (
        <Dimmed on={removing(s)}>
          <AttackBoxPanel
            box={box}
            running={s.running && (!s.deploying || removing(s))}
            host={s.remote ? (status?.host ?? null) : null}
            onShell={shell.open}
            shellReady={s.remote ? s.isDocker : !!box.status?.running}
            controller={!s.isDocker && status?.provider === "qemu"}
          />
        </Dimmed>
      )}

      {!s.running && rt && (
        <WhereItRunsPanel runtime={rt} hostArch={p.hostArch} native={s.native} emulates={s.emulates} isDocker={s.isDocker} meta={meta} cost={cost} />
      )}

      {s.running && (
        <Dimmed on={removing(s)}>
          <DetailsPanel
            status={status}
            engine={s.engine}
            meta={meta}
            cost={cost}
            provision={
              s.canProvision
                ? {
                    target: page.provisionTarget,
                    onTarget: page.setProvisionTarget,
                    busy: busy || s.deploying,
                    running: acting === "provision",
                    onRun: () => void act("provision"),
                  }
                : null
            }
          />
        </Dimmed>
      )}
      {!p.loggedIn && <p className="px-1 text-[0.75rem] text-muted-foreground">{t("labs.detail.signInToRun")}</p>}
    </aside>
  );
}
