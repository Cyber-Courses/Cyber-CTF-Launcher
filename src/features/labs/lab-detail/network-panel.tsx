"use client";

import { Suspense, lazy } from "react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { HostHeader, hasPortTabs } from "@/features/labs/network-diagram/host-header";
import type { Attacker } from "@/features/labs/network-diagram/model";
import type { NetworkNote } from "@/features/labs/lab-detail/lab-state";
import type { Place } from "@/features/labs/place";
import type { AttackStatus, LabStatus } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

// The diagram (React Flow and its cards) loads with the first lab page that draws one, not with
// the app; ELK, for its layout, loads after it (network-diagram/elk-engine.ts). React.lazy rather
// than next/dynamic: the placeholder needs the panel's props.
const NetworkDiagram = lazy(() => import("@/features/labs/network-diagram").then((m) => ({ default: m.NetworkDiagram })));

/** The diagram's card while its code loads: the same header, and the shell at the height the
 *  diagram starts at (before its layout), so the page doesn't move when it arrives. */
function DiagramSkeleton({ place, docked }: { place: Place | null; docked: boolean }) {
  return (
    <div className={cn("hostcard", docked && "hostcard-docked")} aria-busy="true">
      <HostHeader place={place} />
      <div className="topology-shell" />
    </div>
  );
}

/** The attack box as the diagram draws it: as the runner reports it, wherever the lab runs; a
 *  remote lab whose box the launcher doesn't read falls back on its host's report. */
function diagramAttacker(status: LabStatus, remote: boolean, box: AttackStatus | null): Attacker {
  if (box) return { running: box.running, ip: box.ip, labNetwork: box.labNetwork };
  if (remote && status.attacker) return { running: true, ip: status.attacker.ip, labNetwork: status.attacker.labNetwork };
  return null;
}

/** The lab's network: its diagram, or a note saying why there is none yet (see networkView). */
export function NetworkPanel({
  view,
  status,
  place,
  remote,
  box,
  meta,
}: {
  view: "diagram" | NetworkNote;
  status: LabStatus | undefined;
  /** Where the lab runs: the host card's logo and name. */
  place: Place | null;
  remote: boolean;
  /** The local attack box's last status. */
  box: AttackStatus | null;
  /** The runtime word for the header, when the lab has one. */
  meta: string | undefined;
}) {
  const t = useT();
  if (view === "diagram" && status) {
    return (
      <Suspense fallback={<DiagramSkeleton place={place} docked={hasPortTabs(status.machines)} />}>
        <NetworkDiagram machines={status.machines} networks={status.networks} place={place} attacker={diagramAttacker(status, remote, box)} />
      </Suspense>
    );
  }
  return (
    <Panel>
      <PanelHeader title={t("labs.detail.network")} meta={meta} />
      <p className="dotted-canvas px-6 py-14 text-center text-[0.8125rem] text-muted-foreground">{t(`labs.detail.${view as NetworkNote}`)}</p>
    </Panel>
  );
}
