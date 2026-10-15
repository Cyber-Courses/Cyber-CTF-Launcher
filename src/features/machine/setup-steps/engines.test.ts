import { describe, expect, it } from "vitest";
import type { SystemReport } from "@/lib/tauri";
import { engineBadge, engineName, enginesFor, recommendedEngine } from "@/features/machine/setup-steps/engines";

const report = (over: Partial<SystemReport> = {}) => ({ dockerEngine: null, dockerEnginesRunning: [], ...over }) as unknown as SystemReport;

describe("container engines", () => {
  it("offers each OS its engines and recommends the one with the one-click install", () => {
    expect(enginesFor("macos").map((e) => e.id)).toEqual(["docker-desktop", "orbstack", "colima"]);
    expect(enginesFor("windows").map((e) => e.id)).toEqual(["docker-desktop"]);
    expect(enginesFor("linux").map((e) => e.id)).toEqual(["docker-desktop", "docker-engine", "colima"]);
    expect(recommendedEngine("macos")).toBe("docker-desktop");
    expect(recommendedEngine("windows")).toBe("docker-desktop");
    expect(recommendedEngine("linux")).toBe("docker-engine");
  });

  it("badges the engine in use first, then a running one, then the recommended one", () => {
    const r = report({ dockerEngine: "orbstack", dockerEnginesRunning: ["orbstack", "colima"] });
    expect(engineBadge(r, "orbstack", "orbstack")).toBe("in use");
    expect(engineBadge(r, "colima", "colima")).toBe("running");
    expect(engineBadge(r, "docker-desktop", "docker-desktop")).toBe("recommended");
    expect(engineBadge(report(), "docker-desktop", "docker-engine")).toBeUndefined();
  });

  it("names engines, Podman included", () => {
    expect(engineName("podman")).toBe("Podman");
    expect(engineName("colima")).toBe("Colima");
  });
});
