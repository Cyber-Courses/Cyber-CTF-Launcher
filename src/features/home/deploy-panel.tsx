"use client";

import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { StepRow } from "@/features/machine/step-row";
import type { Lab } from "@/features/labs/use-labs";
import { operationLabel } from "@/lib/deploy-store";
import { prettyLabId } from "@/lib/format";
import type { ActiveOperation } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The second column of the Overview: what is being deployed right now, step by step. */
export function DeployPanel({ ops, labs, onOpen }: { ops: ActiveOperation[]; labs: Lab[]; onOpen: (slug: string) => void }) {
  const t = useT();
  const one = ops.length === 1 ? ops[0] : null;
  const labOf = (id: string) => labs.find((l) => l.id === id);
  // The lab's title; its id made readable while the catalogue isn't in (offline).
  const titleOf = (id: string) => labOf(id)?.title ?? prettyLabId(id);
  const oneLab = one ? labOf(one.labId) : undefined;
  return (
    <Panel>
      <PanelHeader
        title={
          <>
            <StatusDot tone="warn" pulse />
            <span className="truncate">{one ? titleOf(one.labId) : t("home.deploy.inProgress")}</span>
          </>
        }
        meta={one ? operationLabel(one).replace(/…$/, "").toLowerCase() : t("home.running.labs", { count: ops.length })}
        action={
          oneLab && (
            <Button variant="ghost" size="xs" onClick={() => onOpen(oneLab.slug)}>
              {t("home.deploy.view")}
            </Button>
          )
        }
      />
      <div className="py-1.5">
        {ops.map((o) => {
          const lab = labOf(o.labId);
          return (
            <StepRow
              key={o.labId}
              state="run"
              label={o.step ?? operationLabel(o)}
              detail={one ? undefined : titleOf(o.labId)}
              meta={o.machine?.replace(/^isoloom-/, "") ?? t("home.deploy.running")}
              action={
                !one &&
                lab && (
                  <Button variant="ghost" size="xs" onClick={() => onOpen(lab.slug)}>
                    {t("home.deploy.view")}
                  </Button>
                )
              }
            />
          );
        })}
      </div>
    </Panel>
  );
}
