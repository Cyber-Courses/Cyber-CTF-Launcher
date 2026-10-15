import { runsNatively } from "@/features/labs/lab-runtime";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

/** What a list row says the lab is doing, most pressing first. */
export type RowPhase = "busy" | "starting" | "running" | "failed" | "paused" | "shutDown" | "solved" | "notStarted";

export function rowPhase({
  status,
  busy,
  deploying,
  solved,
  failed = false,
}: {
  status: LabStatus | undefined;
  /** This session runs an operation on it. */
  busy: boolean;
  /** The backend deploys it (e.g. started before a reload). */
  deploying: boolean;
  solved: boolean;
  /** Its last start, from this window, failed. */
  failed?: boolean;
}): RowPhase {
  const running = status?.running ?? false;
  if (busy) return "busy";
  if (deploying && !running) return "starting";
  if (running) return "running";
  if (failed) return "failed";
  if (status?.parked) return status.parked === "pause" ? "paused" : "shutDown";
  return solved ? "solved" : "notStarted";
}

/** Whether a row's lab runs, is parked (how), and runs emulated here. */
export function rowFacts({
  runtime: rt,
  status,
  hostArch,
  emulates,
}: {
  runtime: LabRuntimeInfo | null;
  status: LabStatus | undefined;
  hostArch: string;
  /** An emulator (QEMU) is ready here. */
  emulates: boolean;
}) {
  // Unknown host (report not in yet) or a lab for any CPU: native, never a wrong "emulated".
  const native = !hostArch || !rt || runsNatively(rt, hostArch);
  const running = status?.running ?? false;
  return {
    // Emulated only where it actually runs here: containers always, VMs with an emulator ready.
    emulated: !native && (rt.runtime !== "VM" || emulates),
    running,
    parked: !running && (status?.parked ?? null),
  };
}

/** The ids of labs running now (the scan or their status says so) that the catalogue doesn't
 *  list: all of them while it can't be loaded. They still run, and still count. */
export function runningUnlisted(labs: Lab[] | null, statuses: Partial<Record<string, LabStatus>>): string[] {
  const listed = new Set((labs ?? []).map((l) => l.id));
  return Object.entries(statuses)
    .filter(([id, s]) => (s?.running ?? false) && !listed.has(id))
    .map(([id]) => id)
    .sort();
}
