"use client";

import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { DetailsPanel } from "@/features/labs/lab-detail/details-panel";
import { removing } from "@/features/labs/lab-detail/lab-state";
import { Dimmed, ObjectivePanel } from "@/features/labs/lab-detail/page-parts";
import type { LabDetailProps, LabPage } from "@/features/labs/lab-detail/use-lab-detail";
import { WhereItRunsPanel } from "@/features/labs/lab-detail/where-it-runs-panel";
import { useT } from "@/lib/i18n";

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
                    onRun: () => {
                      void act("provision");
                    },
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
