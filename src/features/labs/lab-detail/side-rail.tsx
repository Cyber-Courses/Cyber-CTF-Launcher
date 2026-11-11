"use client";

import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { ScreensPanel } from "@/features/labs/screens-panel";
import { DetailsPanel } from "@/features/labs/lab-detail/details-panel";
import { removing } from "@/features/labs/lab-detail/lab-state";
import { Dimmed, ObjectivePanel } from "@/features/labs/lab-detail/page-parts";
import type { LabDetailProps, LabPage } from "@/features/labs/lab-detail/use-lab-detail";
import { WhereItRunsPanel } from "@/features/labs/lab-detail/where-it-runs-panel";
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

/** The side rail: the plain question, the attack box, where it runs (stopped) or its details
 *  (running), and the sign-in note. */
export function LabSideRail({ page, props: p, meta, cost }: { page: LabPage; props: LabDetailProps; meta: string; cost: string | null }) {
  const t = useT();
  const { s, box, shell, screen, busy } = page;
  // The attack box's screen opens once it runs and its start is done, where its run offers one.
  const boxScreen = !!box.status?.running && !box.busy && !!box.status.console && box.status.console.protocol !== "none";
  const { lab, status } = p;
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
            onScreen={
              boxScreen && box.status
                ? () => {
                    screen.open(box.status?.machine ?? "");
                  }
                : undefined
            }
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
          options={page.run.runOptions}
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
          <ScreensPanel machines={status.machines} onOpen={screen.open} error={screen.error} />
        </Dimmed>
      )}
      {!p.loggedIn && !lab.imported && <p className="px-1 text-[0.75rem] text-muted-foreground">{t("labs.detail.signInToRun")}</p>}
    </aside>
  );
}
