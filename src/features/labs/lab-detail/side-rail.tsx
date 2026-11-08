"use client";

import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
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
  const { s, box, shell } = page;
  const { lab, status, busy } = p;
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
            // On a server or cloud account the box comes up with the lab: its shell only once the
            // lab is up, never while it is still starting (or stopping).
            shellReady={s.remote ? s.isDocker && s.running && !s.deploying : !!box.status?.running}
          />
        </Dimmed>
      )}

      {!s.running && lab.runtime && (
        <WhereItRunsPanel
          labId={lab.id}
          runtime={lab.runtime}
          hostArch={p.hostArch}
          readyVms={p.readyVms ?? []}
          native={s.native}
          emulates={s.emulates}
          isDocker={s.isDocker}
          meta={meta}
          cost={cost}
        />
      )}

      {s.running && (
        <Dimmed on={removing(s)}>
          <DetailsPanel status={status} engine={s.engine} meta={meta} cost={cost} provision={provisionControl(page, busy)} />
        </Dimmed>
      )}
      {!p.loggedIn && <p className="px-1 text-[0.75rem] text-muted-foreground">{t("labs.detail.signInToRun")}</p>}
    </aside>
  );
}
