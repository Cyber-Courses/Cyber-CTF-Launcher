"use client";

import { useEffect, useState } from "react";
import { Cpu } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import { runPlaces } from "@/features/labs/lab-runtime";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { labRequirements, type LabRequirements } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useT, type T } from "@/lib/i18n";

/** A lab's minimum-to-run requirements, read from its spec at the catalogue's commit (null until
 *  read, or when the lab has no machines of its own). */
function useLabRequirements(labId: string, runtime: LabRuntimeInfo): LabRequirements | null {
  const [reqs, setReqs] = useState<LabRequirements | null>(null);
  const { repository, commit } = runtime;
  useEffect(() => {
    if (!repository || !commit) return;
    let alive = true;
    labRequirements(labId, repository, commit)
      .then((r) => {
        if (alive) setReqs(r);
      })
      .catch(() => {
        if (alive) setReqs(null);
      });
    return () => {
      alive = false;
    };
  }, [labId, repository, commit]);
  return reqs;
}

/** "2–6 vCPU · 12 GB RAM · 20–60 GB disk" from a lab's lightest-to-heaviest range, a single value
 *  where the ends meet. Memory rounds up to whole GB: the figure is a floor to plan for, not a
 *  measurement. */
function requirementsSummary(t: T, r: LabRequirements): string {
  const range = (min: number, max: number) => (min === max ? String(min) : `${String(min)}–${String(max)}`);
  const gb = (mb: number) => Math.ceil(mb / 1024);
  return t("labs.detail.requirementsSummary", {
    cpu: range(r.min.cpus, r.max.cpus),
    ram: range(gb(r.min.memoryMb), gb(r.max.memoryMb)),
    disk: range(r.min.diskGb, r.max.diskGb),
  });
}

/** Before it runs, the aside would otherwise be empty for a VM lab: say what the lab is and
 *  where it can run, and what it needs, so the page reads as complete at rest. */
export function WhereItRunsPanel({
  labId,
  runtime,
  hostArch,
  native,
  emulates,
  isDocker,
  meta,
  cost = null,
}: {
  labId: string;
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
  const requirements = useLabRequirements(labId, runtime);
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
        {requirements && (
          <div className="flex items-center gap-2.5 border-t border-border px-4 py-2 text-[0.8125rem]">
            <Cpu className="size-3.5 shrink-0 text-faint" />
            <span className="text-muted-foreground">{t("labs.detail.systemRequirements")}</span>
            <span className="ml-auto font-mono text-[0.6875rem] text-foreground">{requirementsSummary(t, requirements)}</span>
          </div>
        )}
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
