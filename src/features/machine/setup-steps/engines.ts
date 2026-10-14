import { type DockerEngine, type SystemReport } from "@/lib/tauri";
import { type MessageKey } from "@/lib/i18n";

// ---------- Container engine ----------

export type Engine = { id: DockerEngine; name: string; /** Translated at render. */ note: MessageKey; url: string; os: string[]; logo: string; tile?: boolean };
/** Docker-compatible engines, per OS. The recommended one gets the one-click install; others link out. */
export const ENGINES: Engine[] = [
  {
    id: "docker-desktop",
    name: "Docker Desktop",
    note: "machine.engine.notes.dockerDesktop",
    url: "https://www.docker.com/products/docker-desktop/",
    os: ["macos", "windows", "linux"],
    logo: "/brands/docker.svg",
  },
  {
    id: "docker-engine",
    name: "Docker Engine",
    note: "machine.engine.notes.dockerEngine",
    url: "https://docs.docker.com/engine/install/",
    os: ["linux"],
    logo: "/brands/docker.svg",
  },
  {
    id: "orbstack",
    name: "OrbStack",
    note: "machine.engine.notes.orbstack",
    url: "https://orbstack.dev/",
    os: ["macos"],
    logo: "/brands/orbstack.png",
    tile: true,
  },
  {
    id: "colima",
    name: "Colima",
    note: "machine.engine.notes.colima",
    url: "https://github.com/abiosoft/colima",
    os: ["macos", "linux"],
    logo: "/brands/colima.png",
  },
];

/** Display name of a Docker-compatible engine. */
export const engineName = (id: DockerEngine) => (id === "podman" ? "Podman" : (ENGINES.find((e) => e.id === id)?.name ?? "Docker"));

/** The engines offered on `os`. */
export const enginesFor = (os: string) => ENGINES.filter((e) => e.os.includes(os));

/** The engine that gets the one-click install: Docker Desktop on macOS and Windows, Docker Engine on Linux. */
export const recommendedEngine = (os: string): DockerEngine => (os === "macos" || os === "windows" ? "docker-desktop" : "docker-engine");

/** The badge on an engine's card: in use, running, recommended or none. */
export function engineBadge(report: SystemReport, id: DockerEngine, recommended: DockerEngine) {
  if (report.dockerEngine === id) return "in use";
  if (report.dockerEnginesRunning?.includes(id)) return "running";
  return id === recommended ? "recommended" : undefined;
}
