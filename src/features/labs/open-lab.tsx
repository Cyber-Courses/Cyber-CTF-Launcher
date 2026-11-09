"use client";

import { useEffect, useState } from "react";
import { Spinner } from "@/components/ui/spinner";
import { LabDetail } from "@/features/labs/lab-detail";
import type { useLabActions } from "@/features/labs/use-lab-actions";
import type { Lab } from "@/features/labs/use-labs";
import { useWatchLab } from "@/features/labs/lab-polling";
import type { LocalVmPick } from "@/features/labs/lab-operations";
import type { RunTarget } from "@/features/labs/run-on";
import type { LabStatus, Provider, SystemReport } from "@/lib/tauri";

// The Labs screen's open lab: which one, from the navigation, and its page.

/** How a chosen target runs beyond plain containers, for the launch: a VM here on a hypervisor, as
 *  one VM per machine here, or a cloud account's one-VM-per-machine form (cloud-vm, no local
 *  hypervisor). Nothing for plain containers, a server, or a cloud account's one-VM form. */
function localVmPick(target: RunTarget): LocalVmPick | undefined {
  // A VM lab here on the hypervisor it ran on (Reset), or the backend's when none is named.
  if (target.kind === "local" && target.provider) return { provider: target.provider };
  if (target.kind === "local-vm") return { provider: target.provider };
  if (target.kind === "local-vms") return { provider: target.provider, perMachine: true };
  if (target.kind === "host" && target.perMachine) return { perMachine: true };
  return undefined;
}

/**
 * The open lab's slug. Opened straight from the slug the navigation carried, so a lab opened
 * from Overview, the command palette or a deep link shows its page on the first render instead
 * of flashing the list first. External navigation (sidebar, command palette, Overview) acts
 * only when the nav tick changes, so a labs refresh never ejects the user from a detail they
 * opened from the list. A slug opens it (once labs load); no slug returns to the list.
 */
export function useOpenLab(openLab: { slug: string | null; tick: number }, labs: Lab[] | null) {
  const [detailSlug, setDetailSlug] = useState<string | null>(() => openLab.slug);
  // The nav tick last acted on, kept as state so a new one is acted on during render, once.
  const [seenTick, setSeenTick] = useState(openLab.tick);
  // A slug that isn't in the catalogue yet: wait (don't consume the tick) until labs load.
  const waiting = openLab.slug && !labs?.some((l) => l.slug === openLab.slug);
  if (openLab.tick !== seenTick && !waiting) {
    setSeenTick(openLab.tick);
    setDetailSlug(openLab.slug);
  }
  return [detailSlug, setDetailSlug] as const;
}

/** Tells the shell the open lab's title (null on the list, and once the screen goes). */
export function useDetailTitle(title: string | null, onDetailChange?: (title: string | null) => void) {
  useEffect(() => onDetailChange?.(title), [title, onDetailChange]);
  useEffect(() => () => onDetailChange?.(null), [onDetailChange]);
}

/** Opening a lab while the catalogue is still loading: a placeholder, never the list, so there
 *  is no flash of the list before the lab page. */
export function OpenLabLoading() {
  return (
    <div className="flex items-center justify-center py-24 text-muted-foreground">
      <Spinner className="size-5" />
    </div>
  );
}

/** The open lab's page, wired to the shared lab actions. */
export function OpenLab({
  lab,
  status,
  actions: { runs, launch, stop, park, resume, provision },
  loggedIn,
  onLogin,
  hostArch,
  readyVms,
  report,
  ready,
  onBack,
}: {
  lab: Lab;
  status: LabStatus | undefined;
  actions: ReturnType<typeof useLabActions>;
  loggedIn: boolean;
  onLogin?: () => Promise<boolean>;
  hostArch: string;
  readyVms: Provider[];
  report?: SystemReport | null;
  ready: boolean;
  onBack: () => void;
}) {
  // Its status is read on every poll while its page is open, whatever state it is in.
  useWatchLab(lab.id);
  return (
    <LabDetail
      lab={lab}
      status={status}
      busy={runs[lab.id]?.busy ?? false}
      logs={runs[lab.id]?.logs ?? []}
      times={runs[lab.id]?.times ?? []}
      runOp={runs[lab.id]?.op}
      runStartedAt={runs[lab.id]?.startedAt}
      runFailureCode={runs[lab.id]?.failureCode}
      runFailureHolder={runs[lab.id]?.failureHolder}
      loggedIn={loggedIn}
      onLogin={onLogin}
      hostArch={hostArch}
      onBack={onBack}
      readyVms={readyVms}
      dockerRunning={report ? report.dockerRunning : null}
      dockerEngine={report ? report.dockerEngine : null}
      dockerEnginesRunning={report ? report.dockerEnginesRunning : []}
      onStart={(target) =>
        launch(
          lab,
          target.kind === "host" ? target.id : null,
          localVmPick(target),
          report,
          target.kind === "local" ? { ports: target.ports, engine: target.engine } : undefined,
        )
      }
      onStop={() => stop(lab)}
      onPark={(mode) => park(lab, mode)}
      onResume={() => resume(lab)}
      onProvision={(machine) => provision(lab, machine)}
      ready={ready}
    />
  );
}
