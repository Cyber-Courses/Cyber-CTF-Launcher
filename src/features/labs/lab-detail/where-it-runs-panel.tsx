"use client";

import { useEffect, useState } from "react";
import { Cpu } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import { runPlaces } from "@/features/labs/lab-runtime";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { labRequirements, type LabRequirements, type LabRunOptions, type Provider, type ServerHost } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useT, type MessageKey, type T } from "@/lib/i18n";
import { warn } from "@/lib/failure";

/** What a place the lab supports but the launcher isn't set up for yet tells the player to do. */
const SETUP_HINT: Partial<Record<string, MessageKey>> = {
  local_vm: "labs.detail.installHypervisor",
  server: "labs.detail.connectServer",
  cloud: "labs.detail.connectAccount",
};

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
      .catch(
        warn("reading the lab's requirements", () => {
          if (alive) setReqs(null);
        }),
      );
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

/** One place in the panel: ready (green), configurable (amber, with what to set up), or
 *  unsupported by the lab at all (faint, "n/a"). */
function PlaceRow({ place, t }: { place: ReturnType<typeof runPlaces>[number]; t: T }) {
  const { key, icon: Icon, state } = place;
  const ready = state === "ready";
  // Supported by the lab, but the launcher isn't set up for it yet: amber, with what to do.
  const configurable = state === "configurable";
  const hint = configurable ? SETUP_HINT[key] : undefined;
  return (
    <div
      className="flex items-center gap-2.5 border-t border-border px-4 py-2 text-[0.8125rem] first:border-t-0"
      title={state === "unsupported" ? t("labs.detail.notAvailable") : hint ? t(hint) : undefined}
    >
      <Icon className={cn("size-3.5 shrink-0", ready ? "text-jewel-text" : configurable ? "text-warning" : "text-faint opacity-50")} />
      <span className={state === "unsupported" ? "text-faint" : "text-foreground"}>{t(`labs.places.${key}`)}</span>
      {hint && <span className="ml-auto font-mono text-[0.6875rem] text-muted-foreground">{t(hint)}</span>}
      {state === "unsupported" && <span className="ml-auto font-mono text-[0.6875rem] text-faint">{t("labs.detail.na")}</span>}
    </div>
  );
}

/** Before it runs, the aside would otherwise be empty for a VM lab: say what the lab is and
 *  where it can run, and what it needs, so the page reads as complete at rest. */
export function WhereItRunsPanel({
  labId,
  runtime,
  hostArch,
  readyVms,
  hosts,
  options,
  native,
  emulates,
  isDocker,
  meta,
  cost = null,
}: {
  labId: string;
  runtime: LabRuntimeInfo;
  hostArch: string;
  /** The local hypervisors ready on this machine, so "VM here" reads as ready, not "install one". */
  readyVms: Provider[];
  /** The saved servers and cloud accounts, and the lab's run options (null while they load): the
   *  page's own reads, which the launch picker shows too. */
  hosts: ServerHost[];
  options: LabRunOptions | null;
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
        {runPlaces(runtime, hostArch, emulates, { readyVms, hosts, options }).map((place) => (
          <PlaceRow key={place.key} place={place} t={t} />
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
