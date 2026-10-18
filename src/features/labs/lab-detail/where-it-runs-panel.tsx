"use client";

import { Panel, PanelHeader } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import { runPlaces } from "@/features/labs/lab-runtime";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** Before it runs, the aside would otherwise be empty for a VM lab: say what the lab is and
 *  where it can run, so the page reads as complete at rest. */
export function WhereItRunsPanel({
  runtime,
  hostArch,
  native,
  emulates,
  isDocker,
  meta,
  cost = null,
}: {
  runtime: LabRuntimeInfo;
  hostArch: string;
  native: boolean;
  emulates: boolean;
  isDocker: boolean;
  meta: string;
  /** A cloud lab's estimated cost (null: none given, or not a cloud lab). */
  cost?: string | null;
}) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.detail.whereItRuns")} meta={meta} />
      <div>
        {runPlaces(runtime, hostArch, emulates).map(({ key, icon: Icon, available }) => (
          <div
            key={key}
            className="flex items-center gap-2.5 border-t border-border px-4 py-2 text-[0.8125rem] first:border-t-0"
            title={available ? undefined : t("labs.detail.notAvailable")}
          >
            <Icon className={cn("size-3.5 shrink-0", available ? "text-jewel-text" : "text-faint opacity-50")} />
            <span className={available ? "text-foreground" : "text-faint"}>{t(`labs.places.${key}`)}</span>
            {!available && <span className="ml-auto font-mono text-[0.6875rem] text-faint">{t("labs.detail.na")}</span>}
          </div>
        ))}
        {runtime.runtime === "CLOUD" && (
          <div className="border-t border-border px-4 py-2.5 text-[0.75rem] leading-relaxed text-muted-foreground">
            {cost ? t("labs.cloud.billedEstimate", { cost }) : t("labs.cloud.billedWhileRunning")}
          </div>
        )}
        {!native && (
          <div className="flex items-center gap-2.5 border-t border-border px-4 py-2.5 text-[0.75rem] text-muted-foreground">
            <StatusDot tone="warn" /> {isDocker ? t("labs.detail.emulatedContainers") : emulates ? t("labs.detail.emulatedQemu") : t("labs.detail.foreignCpu")}
          </div>
        )}
      </div>
    </Panel>
  );
}
