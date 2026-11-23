import { type DockerEngine, type SystemReport } from "@/lib/tauri";
// Values with no command behind them: read where they are generated, so a test that stands in for
// the commands still has them.
import { DOCKER_ENGINES, DOCKER_ENGINE_NAMES, DOCKER_ENGINE_URLS } from "@/lib/tauri/generated/DockerEngines";
import { type MessageKey } from "@/lib/i18n";

// ---------- Container engine ----------

/** Display name of a Docker-compatible engine (the backend's). */
export const engineName = (id: DockerEngine) => DOCKER_ENGINE_NAMES[id];

export type Engine = { id: DockerEngine; name: string; /** Translated at render. */ note: MessageKey; url: string; logo: string; tile?: boolean };
/** How the setup shows the Docker-compatible engines it offers, named and linked as the backend
 *  names them. Which are offered on this OS, in which order, and what each takes here, are the
 *  core's (the report's `setup.engines`: [`enginesFor`]). */
export const ENGINES: Engine[] = (
  [
    { id: "docker-desktop", note: "machine.engine.notes.dockerDesktop", logo: "/brands/docker.svg" },
    { id: "docker-engine", note: "machine.engine.notes.dockerEngine", logo: "/brands/docker.svg" },
    { id: "orbstack", note: "machine.engine.notes.orbstack", logo: "/brands/orbstack.png", tile: true },
    { id: "colima", note: "machine.engine.notes.colima", logo: "/brands/colima.png" },
  ] satisfies Omit<Engine, "name" | "url">[]
).map((e) => ({ ...e, name: engineName(e.id), url: DOCKER_ENGINE_URLS[e.id] }));

/** Whether `id` (a lab status's provider, say) names a Docker engine the backend knows. */
export const isDockerEngine = (id: string): id is DockerEngine => (DOCKER_ENGINES as readonly string[]).includes(id);

/** The engines the core offers on this machine, in its order (the report's `setup.engines`). */
export const enginesFor = (report: SystemReport): Engine[] =>
  report.setup.engines.map((o) => ENGINES.find((e) => e.id === o.engine)).filter((e): e is Engine => e !== undefined);

/** The badge on an engine's card: in use, running, recommended or none. */
export function engineBadge(report: SystemReport, id: DockerEngine, recommended: DockerEngine) {
  if (report.dockerEngine === id) return "in use";
  if (report.dockerEnginesRunning.includes(id)) return "running";
  return id === recommended ? "recommended" : undefined;
}
