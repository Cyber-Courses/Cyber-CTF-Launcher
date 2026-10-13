import type { LabStatus, SystemReport } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";
import type { MessageKey } from "@/lib/i18n";

// What the Overview shows, as pure functions of the labs, their statuses and this machine.

/** "Good morning / afternoon / evening" from the local hour. */
export const greetingKey = (hour: number): MessageKey =>
  hour < 12 ? "home.greeting.morning" : hour < 18 ? "home.greeting.afternoon" : "home.greeting.evening";

/** Whether Docker is ready: unknown (null) until the machine report is in, so there's no "set up"
 *  call to action for a Docker that is simply not probed yet. */
export const dockerReadiness = (report: SystemReport | null) => (report ? report.docker.installed && report.dockerRunning : null);

/** The line under the greeting (home.status.*). `checkFailed`: the machine check failed, and none
 *  answered before it (the report is unknown). */
export function heroStatus(
  dockerReady: boolean | null,
  running: number,
  checkFailed = false,
): "checking" | "checkFailed" | "setUpDocker" | "running" | "ready" {
  if (dockerReady === null) return checkFailed ? "checkFailed" : "checking";
  if (!dockerReady) return "setUpDocker";
  return running > 0 ? "running" : "ready";
}

/** The labs running now. */
export const runningLabs = (labs: Lab[], statuses: Record<string, LabStatus>) => labs.filter((l) => statuses[l.id]?.running);

/** The running labs' own containers: the engine's total also counts the player's other projects. */
export const labContainerCount = (running: Lab[], statuses: Record<string, LabStatus>) =>
  running
    .filter((l) => l.runtime?.runtime === "DOCKER" && (statuses[l.id]?.place ?? "container") === "container")
    .reduce((n, l) => n + (statuses[l.id]?.machines.length ?? 0), 0);

/** "Jump back in": recently launched labs, most recent first, not already running. */
export function recentLabs(labs: Lab[], statuses: Record<string, LabStatus>, lastRun: (id: string) => number | null, max = 3) {
  return labs
    .map((lab) => ({ lab, ts: lastRun(lab.id) }))
    .filter((r): r is { lab: Lab; ts: number } => r.ts !== null && !statuses[r.lab.id]?.running)
    .sort((a, b) => b.ts - a.ts)
    .slice(0, max);
}
