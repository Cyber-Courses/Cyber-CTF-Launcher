import { CLOUDS } from "@/features/labs/lab-runtime";
import type { Lab } from "@/features/labs/use-labs";

export type StatusFilter = "all" | "todo" | "running" | "solved";
/** CLOUD = cloud-services labs, and the labs that can run in the player's cloud account. */
export type RuntimeFilter = "all" | "DOCKER" | "VM" | "CLOUD";
/** The Labs list's search and filters; `difficulty` 0 = any level. */
export type LabFilters = { query: string; status: StatusFilter; runtime: RuntimeFilter; difficulty: number };

export const NO_FILTERS: LabFilters = { query: "", status: "all", runtime: "all", difficulty: 0 };

/** The labs the search and filters keep: the query matches the title, category, description or
 *  a skill; "to do" is neither solved nor running. */
export function filterLabs(labs: Lab[], f: LabFilters, isRunning: (lab: Lab) => boolean, completed: Set<string>): Lab[] {
  const q = f.query.trim().toLowerCase();
  return labs.filter((l) => {
    if (q && !`${l.title} ${l.category} ${l.description ?? ""} ${(l.skills ?? []).map((s) => s.name).join(" ")}`.toLowerCase().includes(q)) return false;
    if (!runtimeMatches(l, f.runtime)) return false;
    if (f.difficulty && l.difficulty !== f.difficulty) return false;
    const running = isRunning(l);
    if (f.status === "running" && !running) return false;
    if (f.status === "solved" && !completed.has(l.id)) return false;
    if (f.status === "todo" && (completed.has(l.id) || running)) return false;
    return true;
  });
}

/** Whether a lab passes the runtime filter: "Cloud" keeps cloud services and the labs a cloud
 *  account can run. */
function runtimeMatches(lab: Lab, runtime: RuntimeFilter): boolean {
  if (runtime === "all") return true;
  if (runtime === "CLOUD") return lab.runtime?.runtime === "CLOUD" || !!lab.runtime?.providers.some((p) => CLOUDS.has(p));
  return lab.runtime?.runtime === runtime;
}
