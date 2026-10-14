import { describe, expect, it } from "vitest";
import type { LabStatus, SystemReport } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";
import { dockerReadiness, greetingKey, heroStatus, labContainerCount, recentLabs } from "@/features/home/home-model";

const lab = (id: string, runtime: "DOCKER" | "VM" = "DOCKER") => ({ id, slug: id, title: id, runtime: { runtime } }) as Lab;
const status = (running: boolean, patch: Partial<LabStatus> = {}) =>
  ({ running, place: null, machines: [{}, {}, {}] as LabStatus["machines"], ...patch }) as LabStatus;

describe("home model: edges", () => {
  it("greets by the hour at each boundary, and in the evening for an hour it cannot place", () => {
    expect(greetingKey(0)).toBe("home.greeting.morning");
    expect(greetingKey(11)).toBe("home.greeting.morning");
    expect(greetingKey(12)).toBe("home.greeting.afternoon");
    expect(greetingKey(17)).toBe("home.greeting.afternoon");
    expect(greetingKey(18)).toBe("home.greeting.evening");
    expect(greetingKey(23)).toBe("home.greeting.evening");
    expect(greetingKey(NaN)).toBe("home.greeting.evening");
  });

  it("asks to set up Docker before counting running labs, and installed but stopped is not ready", () => {
    expect(heroStatus(false, 3)).toBe("setUpDocker");
    expect(heroStatus(null, 3)).toBe("checking");
    expect(heroStatus(true, 0)).toBe("ready");
    expect(dockerReadiness({ docker: { installed: true, version: null }, dockerRunning: false } as SystemReport)).toBe(false);
  });

  it("counts containers only for container labs running as containers, none without a status", () => {
    const labs = [lab("a"), lab("b"), lab("c"), lab("d", "VM")];
    const statuses = { a: status(true, { place: "local_vm" }), b: status(true), d: status(true) };
    expect(labContainerCount(labs, statuses)).toBe(3);
  });

  it("keeps the catalogue order for labs launched at the same time, and lists none for a limit of 0", () => {
    const labs = [lab("x"), lab("y"), lab("z")];
    const same = () => 42;
    expect(recentLabs(labs, {}, same).map((r) => r.lab.id)).toEqual(["x", "y", "z"]);
    expect(recentLabs(labs, {}, same, 0)).toEqual([]);
  });
});
