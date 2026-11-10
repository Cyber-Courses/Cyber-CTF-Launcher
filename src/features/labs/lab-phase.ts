import type { LabStatus } from "@/lib/tauri";

/** Where a lab stands in its lifecycle, as the list row and the lab page both say it. */
export type LifecyclePhase = "busy" | "running" | "starting" | "failed" | "paused" | "shutDown";

/** The lab's lifecycle phase, most pressing first: an operation on it (`busy`), up, coming up, its
 *  last start failed, parked (paused or shut down); null for none of these, which the row and the
 *  page each name their own way (solved, interrupted, not started). */
export function lifecyclePhase({
  busy,
  running,
  starting,
  failed,
  parked,
}: {
  busy: boolean;
  running: boolean;
  /** Being deployed (a launch, or one the backend still runs after a reload). */
  starting: boolean;
  failed: boolean;
  parked: LabStatus["parked"] | undefined;
}): LifecyclePhase | null {
  if (busy) return "busy";
  if (running) return "running";
  if (starting) return "starting";
  // Its start just failed: say so, not "Not started" (or "Interrupted", for what it left).
  if (failed) return "failed";
  if (parked) return parked === "pause" ? "paused" : "shutDown";
  return null;
}
