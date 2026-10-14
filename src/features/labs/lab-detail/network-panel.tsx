"use client";

import { Suspense, lazy } from "react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { HostHeader, hasPortTabs } from "@/features/labs/network-diagram/host-header";
import type { Attacker } from "@/features/labs/network-diagram/model";
import type { NetworkNote } from "@/features/labs/lab-detail/lab-state";
import type { ExegolStatus, LabStatus } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

// The diagram (React Flow and its cards) loads with the first lab page that draws one, not with
// the app; ELK, for its layout, loads after it (network-diagram/elk-engine.ts). React.lazy rather
// than next/dynamic: the placeholder needs the panel's props.
const NetworkDiagram = lazy(() => import("@/features/labs/network-diagram").then((m) => ({ default: m.NetworkDiagram })));

/** The diagram's card while its code loads: the same header, and the shell at the height the
 *  diagram starts at (before its layout), so the page doesn't move when it arrives. */
function DiagramSkeleton({ host, docked }: { host: string | null; docked: boolean }) {
  return (
    <div className={cn("hostcard", docked && "hostcard-docked")} aria-busy="true">
      <HostHeader host={host} />
      <div className="topology-shell" />
    </div>
  );
}

/** The attack box as the diagram draws it. A server or cloud lab runs its attack box on the lab
 *  host, which only that host's report sees; a local one reports itself. */
function diagramAttacker(status: LabStatus, remote: boolean, box: ExegolStatus | null): Attacker {
  if (remote) return status.attacker ? { running: true, ip: status.attacker.ip, labNetwork: status.attacker.labNetwork } : null;
  return box ? { running: box.running, ip: box.ip, labNetwork: box.labNetwork } : null;
}

/** The lab's network: its diagram, or a note saying why there is none yet (see networkView). */
export function NetworkPanel({
  view,
  status,
  remote,
  box,
  meta,
}: {
  view: "diagram" | NetworkNote;
  status: LabStatus | undefined;
  remote: boolean;
  /** The local attack box's last status. */
  box: ExegolStatus | null;
  /** The runtime word for the header, when the lab has one. */
  meta: string | undefined;
}) {
  const t = useT();
  if (view === "diagram" && status) {
    return (
      <Suspense fallback={<DiagramSkeleton host={status.host} docked={hasPortTabs(status.machines)} />}>
        <NetworkDiagram machines={status.machines} networks={status.networks} host={status.host} attacker={diagramAttacker(status, remote, box)} />
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
