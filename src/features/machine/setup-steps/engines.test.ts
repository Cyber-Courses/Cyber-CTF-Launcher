import { describe, expect, it } from "vitest";
import { DOCKER_ENGINES, type SystemReport } from "@/lib/tauri";
import { engineBadge, engineName, enginesFor } from "@/features/machine/setup-steps/engines";
import { SETUP_SHAPES } from "@/lib/tauri/generated/SetupShapes";

const report = (over: Partial<SystemReport> = {}) => ({ dockerEngine: null, dockerEnginesRunning: [], ...over }) as unknown as SystemReport;

describe("container engines", () => {
  it("offers the engines the core offers on the machine, in its order, each with its vendor's link", () => {
    expect(enginesFor(SETUP_SHAPES.macReady).map((e) => e.id)).toEqual(["docker-desktop", "orbstack", "colima"]);
    expect(enginesFor(SETUP_SHAPES.bareWindows).map((e) => e.id)).toEqual(["docker-desktop"]);
    expect(enginesFor(SETUP_SHAPES.bareDebian).map((e) => e.id)).toEqual(["docker-desktop", "docker-engine", "colima"]);
    expect(enginesFor(SETUP_SHAPES.bareDebian).every((e) => e.url.startsWith("https://"))).toBe(true);
  });

  it("badges the engine in use first, then a running one, then the recommended one", () => {
    const r = report({ dockerEngine: "orbstack", dockerEnginesRunning: ["orbstack", "colima"] });
    expect(engineBadge(r, "orbstack", "orbstack")).toBe("in use");
    expect(engineBadge(r, "colima", "colima")).toBe("running");
    expect(engineBadge(r, "docker-desktop", "docker-desktop")).toBe("recommended");
    expect(engineBadge(report(), "docker-desktop", "docker-engine")).toBeUndefined();
  });

  it("names every engine the backend names, Podman and Rancher Desktop included", () => {
    expect(engineName("podman")).toBe("Podman");
    expect(engineName("colima")).toBe("Colima");
    expect(engineName("rancher-desktop")).toBe("Rancher Desktop");
    expect(DOCKER_ENGINES.map(engineName).every((name) => name.length > 0 && name !== "Docker")).toBe(true);
  });
});
