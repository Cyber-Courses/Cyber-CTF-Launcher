import { type DockerEngine, type SystemReport } from "@/lib/tauri";
// Values with no command behind them: read where they are generated, so a test that stands in for
// the commands still has them.
import { DOCKER_ENGINES, DOCKER_ENGINE_NAMES } from "@/lib/tauri/generated/DockerEngines";
import { type MessageKey } from "@/lib/i18n";

// ---------- Container engine ----------

/** Display name of a Docker-compatible engine (the backend's). */
export const engineName = (id: DockerEngine) => DOCKER_ENGINE_NAMES[id];

export type Engine = { id: DockerEngine; name: string; /** Translated at render. */ note: MessageKey; url: string; os: string[]; logo: string; tile?: boolean };
/** The Docker-compatible engines offered to install, per OS, named as the backend names them. The
 *  recommended one gets the one-click install; others link out. */
export const ENGINES: Engine[] = (
  [
    {
      id: "docker-desktop",
      note: "machine.engine.notes.dockerDesktop",
      url: "https://www.docker.com/products/docker-desktop/",
      os: ["macos", "windows", "linux"],
      logo: "/brands/docker.svg",
    },
    {
      id: "docker-engine",
      note: "machine.engine.notes.dockerEngine",
      url: "https://docs.docker.com/engine/install/",
      os: ["linux"],
      logo: "/brands/docker.svg",
    },
    {
      id: "orbstack",
      note: "machine.engine.notes.orbstack",
      url: "https://orbstack.dev/",
      os: ["macos"],
      logo: "/brands/orbstack.png",
      tile: true,
    },
    {
      id: "colima",
      note: "machine.engine.notes.colima",
      url: "https://github.com/abiosoft/colima",
      os: ["macos", "linux"],
      logo: "/brands/colima.png",
    },
  ] satisfies Omit<Engine, "name">[]
).map((e) => ({ ...e, name: engineName(e.id) }));

/** Whether `id` (a lab status's provider, say) names a Docker engine the backend knows. */
export const isDockerEngine = (id: string): id is DockerEngine => (DOCKER_ENGINES as readonly string[]).includes(id);

/** The engines offered on `os`. */
export const enginesFor = (os: string) => ENGINES.filter((e) => e.os.includes(os));

/** The engine that gets the one-click install: Docker Desktop on macOS and Windows, Docker Engine on Linux. */
export const recommendedEngine = (os: string): DockerEngine => (os === "macos" || os === "windows" ? "docker-desktop" : "docker-engine");

/** The badge on an engine's card: in use, running, recommended or none. */
export function engineBadge(report: SystemReport, id: DockerEngine, recommended: DockerEngine) {
  if (report.dockerEngine === id) return "in use";
  if (report.dockerEnginesRunning.includes(id)) return "running";
  return id === recommended ? "recommended" : undefined;
}
