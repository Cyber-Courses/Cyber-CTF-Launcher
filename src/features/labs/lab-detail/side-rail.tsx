"use client";

import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { ScreensPanel } from "@/features/labs/screens-panel";
import { DetailsPanel } from "@/features/labs/lab-detail/details-panel";
import { removing } from "@/features/labs/lab-detail/lab-state";
import { Dimmed, ObjectivePanel } from "@/features/labs/lab-detail/page-parts";
import type { LabDetailProps, LabPage } from "@/features/labs/lab-detail/use-lab-detail";
import { WhereItRunsPanel } from "@/features/labs/lab-detail/where-it-runs-panel";
import { RemoveLabPanel } from "@/features/labs/lab-detail/remove-lab-panel";
import { useRemoveLab } from "@/features/labs/lab-detail/use-remove-lab";
import { useT } from "@/lib/i18n";

/** The DetailsPanel's "run provisioning again" control, when this lab can be provisioned again. */
function provisionControl(page: LabPage, busy: boolean) {
  const { s, acting, act } = page;
  if (!s.canProvision) return null;
  return {
    target: page.provisionTarget,
    onTarget: page.setProvisionTarget,
    busy: busy || s.deploying,
    running: acting === "provision",
    onRun: () => {
      void act("provision");
    },
  };
}

/** The attack box's ways into its screen, once it runs and its start is done: the built-in
 *  viewer where its run offers a screen that answers now, and its hypervisor's own app where this
 *  computer has it. */
function boxWays({ box, screen }: LabPage): { onScreen?: () => void; nativeApp: string | null; onNative?: () => void } {
  const up = !!box.status?.running && !box.busy;
  const machine = box.status?.machine ?? "";
  const c = box.status?.console ?? null;
  const shows = up && !!c && c.protocol !== "none" && !c.unavailable;
  const nativeApp = up ? (c?.nativeApp ?? null) : null;
  return {
    onScreen: shows
      ? () => {
          screen.open(machine);
        }
      : undefined,
    nativeApp,
    onNative: nativeApp
      ? () => {
          screen.openNative(machine);
        }
      : undefined,
  };
}

/** The side rail: the plain question, the attack box, where it runs (stopped) or its details
 *  (running), the lab's files here (Remove lab, once it is installed), and the sign-in note. */
export function LabSideRail({ page, props: p, meta, cost }: { page: LabPage; props: LabDetailProps; meta: string; cost: string | null }) {
  const t = useT();
  const { s, box, shell, screen, busy } = page;
  const ways = boxWays(page);
  const { lab, status } = p;
  // Read again as the lab's state changes: a start installs a catalogue lab.
  const removal = useRemoveLab(lab, s.phase, p.onBack);
  // Nothing of it may be up, nor an operation run on it: stop it first.
  const blocked = s.running || s.deploying || !!s.parked || s.interrupted || busy;
  return (
    <aside className="h-fit min-w-0 space-y-5 lg:sticky lg:top-2">
      {/* The objectives couldn't be read: why, and the lab's question in their place. */}
      {page.objectives.state.kind === "unavailable" && <ObjectivePanel question={lab.question} unread={page.objectives.state.why} />}

      {/* VM labs on a server host have no attacker yet (their networks live on that host). */}
      {!s.isCloud && (s.isDocker || !s.remote) && (
        <Dimmed on={removing(s)}>
          <AttackBoxPanel
            box={box}
            running={s.running && (!s.deploying || removing(s))}
            host={s.remote ? (status?.host ?? null) : null}
            onShell={shell.open}
            // The shell only into a box that runs, wherever the lab is, once its start is done (a
            // box made as a machine of the lab is set up during that start).
            shellReady={!!box.status?.running && !box.busy}
            onScreen={ways.onScreen}
            nativeApp={ways.nativeApp}
            onNative={ways.onNative}
          />
        </Dimmed>
      )}

      {!s.running && lab.runtime && (
        <WhereItRunsPanel
          labId={lab.id}
          runtime={lab.runtime}
          hostArch={p.hostArch}
          readyVms={p.readyVms ?? []}
          hosts={page.run.hosts}
          optionsRead={page.run.runOptionsRead}
          native={s.native}
          emulates={s.emulates}
          isDocker={s.isDocker}
          meta={meta}
          cost={cost}
        />
      )}

      {s.running && (
        <Dimmed on={removing(s)}>
          <DetailsPanel status={status} place={page.place} meta={meta} cost={cost} provision={provisionControl(page, busy)} />
        </Dimmed>
      )}

      {s.running && status && (
        <Dimmed on={removing(s)}>
          <ScreensPanel machines={status.machines} onOpen={screen.open} onOpenNative={screen.openNative} error={screen.error} />
        </Dimmed>
      )}
      {removal.installed && <RemoveLabPanel imported={!!lab.imported} blocked={blocked} removing={removal.removing} onRemove={() => void removal.remove()} />}
      {!p.loggedIn && !lab.imported && <p className="px-1 text-[0.75rem] text-muted-foreground">{t("labs.detail.signInToRun")}</p>}
    </aside>
  );
}
