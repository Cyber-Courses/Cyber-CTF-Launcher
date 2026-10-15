"use client";

import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { StepRow } from "@/features/machine/step-row";
import type { Lab } from "@/features/labs/use-labs";
import { operationLabel } from "@/features/labs/runs/operations";
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
            <Button
              variant="ghost"
              size="xs"
              onClick={() => {
                onOpen(oneLab.slug);
              }}
            >
              {t("home.deploy.view")}
            </Button>
          )
        }
      />
      <div className="py-1.5">
        {ops.map((o) => (
          // With one operation, the header names its lab and links to it; with more, each row does.
          <OperationRow key={o.labId} op={o} lab={one ? undefined : (labOf(o.labId) ?? null)} title={titleOf(o.labId)} onOpen={onOpen} />
        ))}
      </div>
    </Panel>
  );
}

/** One operation in flight, at its step: with its lab's title and a link to it, unless `lab` is
 *  undefined (the header has them); `lab` null is one the catalogue doesn't have (no link). */
function OperationRow({ op, lab, title, onOpen }: { op: ActiveOperation; lab: Lab | null | undefined; title: string; onOpen: (slug: string) => void }) {
  const t = useT();
  return (
    <StepRow
      state="run"
      label={op.step ?? operationLabel(op)}
      detail={lab === undefined ? undefined : title}
      meta={op.machine?.replace(/^isoloom-/, "") ?? t("home.deploy.running")}
      action={
        lab && (
          <Button
            variant="ghost"
            size="xs"
            onClick={() => {
              onOpen(lab.slug);
            }}
          >
            {t("home.deploy.view")}
          </Button>
        )
      }
    />
  );
}
