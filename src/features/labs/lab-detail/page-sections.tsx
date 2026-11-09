"use client";

import { useState } from "react";
import { Shuffle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { HealthBanner } from "@/features/labs/lab-health";
import { CloudNotePanel, CloudPanel } from "@/features/labs/cloud-panel";
import { DeploymentPanel } from "@/features/labs/lab-detail/deployment-panel";
import { HeaderActions } from "@/features/labs/lab-detail/header-actions";
import { deployDestination } from "@/features/labs/lab-detail/labels";
import { cloudView, networkView, removing, type HeaderMode } from "@/features/labs/lab-detail/lab-state";
import { FailureNotice, ReapFailedNotice } from "@/features/labs/lab-detail/failure-notice";
import { NetworkPanel } from "@/features/labs/lab-detail/network-panel";
import { Dimmed } from "@/features/labs/lab-detail/page-parts";
import { ObserversPanel } from "@/features/labs/lab-detail/observers-panel";
import { StartLabButton } from "@/features/labs/lab-detail/start-lab-button";
import type { LabDetailProps, LabPage } from "@/features/labs/lab-detail/use-lab-detail";
import { ObjectivesLoading, ObjectivesPanel } from "@/features/labs/objectives/objectives-panel";
import { useStartAfterSignIn } from "@/features/labs/lab-detail/use-start-after-sign-in";
import { useT } from "@/lib/i18n";

// The sections of a lab's page: the header's actions, the notices under it, the deployment and
// the main column's panels (the side rail is in side-rail).

/** The header's one thing to do next: Start (asking where first), or what the lab's state allows. */
export function LabHeaderActions({ mode, page, props: p, onRemove }: { mode: HeaderMode; page: LabPage; props: LabDetailProps; onRemove: () => void }) {
  const t = useT();
  const { s, acting, act } = page;
  const afterSignIn = useStartAfterSignIn(page, p);
  if (mode === "start" || mode === "failed")
    return (
      <>
        {/* A port taken by a container started here: the same start on random free ports is the
            fix, one click away. Not for a VM here (its Vagrantfile forwards the lab's fixed host
            ports, with no random option) nor a server run (its ports aren't on this machine):
            there, freeing the port is the only fix, and this button would wrongly switch the run
            to a plain container. */}
        {mode === "failed" && s.isDocker && s.failureKind === "ports" && !s.remote && s.place !== "local_vm" && (
          <Button
            variant="outline"
            onClick={() => {
              page.startOn({ kind: "local", ports: "random" });
            }}
            disabled={p.busy || !p.loggedIn}
          >
            <Shuffle className="size-3.5" /> {t("labs.detail.retryRandomPorts")}
          </Button>
        )}
        <StartLabButton
          // Remounted when a sign-in from this header went through: the "where" question opens
          // at once when there is one (see useStartAfterSignIn).
          key={afterSignIn.tick}
          chooseAtOnce={afterSignIn.tick > 0}
          retry={mode === "failed"}
          lab={p.lab}
          run={page.run}
          isDocker={s.isDocker}
          cloud={s.isCloud ? { preview: page.preview } : null}
          loggedIn={p.loggedIn}
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
      boxBusy={page.box.busy}
      acting={acting}
      resetting={page.resetting}
      onAct={(what) => void act(what)}
      onRemove={onRemove}
      onStop={p.onStop}
      onLogin={afterSignIn.login}
      startedAt={s.startedAt}
    />
  );
}

/** Under the header: the last operation's failure, a shell that failed to open, and the health
 *  banner (a container went down: say so, check it, offer a clean restart). */
export function LabNotices({ page, props: p }: { page: LabPage; props: LabDetailProps }) {
  const { s, shell, act } = page;
  // Dismissed for this failure only: the next one shows again.
  const failure = s.failure && s.failedOp ? { op: s.failedOp, message: s.failure, key: `${s.failedOp}|${String(s.startedAt ?? "")}|${s.failure}` } : null;
  const [dismissed, setDismissed] = useState<string | null>(null);
  return (
    <>
      {failure && failure.key !== dismissed && (
        <FailureNotice
          title={p.lab.title}
          op={failure.op}
          message={failure.message}
          kind={s.failureKind}
          holder={s.failureHolder}
          // Random free ports fix a container clash here, but not a VM here (fixed forwarded ports)
          // nor a server run: there the hint is to free the port, not to randomise it.
          canRandomPorts={s.isDocker && !s.remote && s.place !== "local_vm"}
          stillUp={failure.op === "stop" && s.running}
          onDismiss={() => {
            setDismissed(failure.key);
          }}
        />
      )}
      {p.status?.reapError && <ReapFailedNotice title={p.lab.title} error={p.status.reapError} />}
      {shell.error && (
        <p className="-mt-2 flex items-center gap-2 text-[0.75rem] text-muted-foreground">
          <StatusDot tone="fail" /> {shell.error}
        </p>
      )}
      {s.running && s.down.length > 0 && (
        <HealthBanner
          down={s.down.map((m) => m.name)}
          check={page.check}
          busy={p.busy}
          resetting={page.resetting}
          onReset={() => {
            void page.reset();
          }}
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
      where={deployDestination(t, p.status, run.runOn, run.hosts, { engine: s.engine, inLocalVm: s.isDocker && s.place === "local_vm" })}
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
      <NetworkPanel view={view} status={p.status} place={page.place} box={page.box.status} meta={p.lab.runtime ? meta : undefined} />
    </Dimmed>
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
      key={`${lab.id}:${String(objectives.state.progressRead)}`}
      challenges={objectives.state.challenges}
      progress={objectives.state.progress}
      signedIn={objectives.state.signedIn}
      actions={objectives}
    />
  );
}
