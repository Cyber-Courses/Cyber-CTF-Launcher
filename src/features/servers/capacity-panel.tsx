"use client";

import { Panel, PanelHeader } from "@/components/ui/panel";
import { Meter } from "@/components/ui/meter";
import { formatBytes } from "@/lib/format";
import type { HostCapacity, ServerHost } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** A server's headroom: its cores and how much of its memory is in use. */
export function CapacityPanel({ host, cap }: { host: ServerHost; cap: HostCapacity }) {
  const t = useT();
  const used = cap.memTotal - cap.memFree;
  return (
    <Panel>
      <PanelHeader title={t("servers.screen.capacityTitle", { name: host.name })} meta={t("servers.screen.vcpu", { count: cap.cores })} />
      <div className="grid gap-3 p-4">
        <div className="grid grid-cols-[6rem_minmax(0,1fr)_auto] items-center gap-4 text-[0.8125rem]">
          <span className="text-muted-foreground">{t("servers.screen.memory")}</span>
          <Meter value={cap.memTotal > 0 ? (used / cap.memTotal) * 100 : 0} />
          <span className="text-right font-mono text-[0.6875rem] text-faint">
            {t("servers.screen.memoryUsed", { used: formatBytes(used), total: formatBytes(cap.memTotal) })}
          </span>
        </div>
      </div>
    </Panel>
  );
}
