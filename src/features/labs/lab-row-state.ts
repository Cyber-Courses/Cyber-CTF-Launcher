import { lifecyclePhase } from "@/features/labs/lab-phase";
import { runsNativelyHere } from "@/features/labs/lab-runtime";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

/** What a list knows of a row's lab beside its status, for the row to read in one piece. */
export type RowSituation = {
  /** This session runs an operation on it. */
  busy: boolean;
  /** The operation in flight while `busy` (launch, stop, shutdown…), for its label. */
  operation?: string;
  /** The backend deploys it (e.g. started before a reload), for the dot. */
  deploying: boolean;
  /** The player already solved it (website evidence). */
  solved: boolean;
  /** Its last start, from this window, failed (see `startFailed`): a red dot, "Failed to start". */
  failed: boolean;
  /** What this machine is missing to run it (see lab-readiness), or null. */
  setup: string | null;
  /** A cloud lab with no account of its cloud yet: the setup action connects one. */
  connectCloud: boolean;
};

/** A lab nothing is happening to, that this machine can run. */
export const IDLE_ROW: RowSituation = { busy: false, deploying: false, solved: false, failed: false, setup: null, connectCloud: false };

/** What a list row says the lab is doing, most pressing first. */
export type RowPhase = "busy" | "starting" | "running" | "failed" | "paused" | "shutDown" | "solved" | "notStarted";

export function rowPhase({
  status,
  busy,
  deploying,
  solved,
  failed = false,
}: Pick<RowSituation, "busy" | "deploying" | "solved"> & Partial<Pick<RowSituation, "failed">> & { status: LabStatus | undefined }): RowPhase {
  const phase = lifecyclePhase({ busy, running: status?.running ?? false, starting: deploying, failed, parked: status?.parked });
  return phase ?? (solved ? "solved" : "notStarted");
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
  const native = runsNativelyHere(rt, hostArch);
  const running = status?.running ?? false;
  return {
    // Emulated only where it actually runs here: containers always, VMs with an emulator ready.
    emulated: !native && (rt?.runtime !== "VM" || emulates),
    running,
    parked: !running && (status?.parked ?? null),
  };
}

/** The ids of labs running now that the catalogue doesn't list: all of them while it can't be
 *  loaded. They still run, and still count. A lab whose status was read goes by it; one never
 *  read (the catalogue doesn't list it, so nothing reads it) by what the scan (`scanned`) sees. */
export function runningUnlisted(labs: Lab[] | null, statuses: Partial<Record<string, LabStatus>>, scanned: Set<string>): string[] {
  const listed = new Set((labs ?? []).map((l) => l.id));
  const ids = new Set([...Object.keys(statuses), ...scanned]);
  return [...ids].filter((id) => !listed.has(id) && (statuses[id]?.running ?? scanned.has(id))).sort();
}
