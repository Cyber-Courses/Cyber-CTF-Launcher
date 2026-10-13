import { describe, expect, it } from "vitest";
import type { LabStatus, SystemReport } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";
import { dockerReadiness, greetingKey, heroStatus, labContainerCount, recentLabs, runningLabs } from "@/features/home/home-model";

const lab = (id: string, runtime: "DOCKER" | "VM" = "DOCKER") => ({ id, slug: id, title: id, runtime: { runtime } }) as Lab;
const status = (running: boolean, patch: Partial<LabStatus> = {}) =>
  ({ running, place: null, machines: [{}, {}] as LabStatus["machines"], ...patch }) as LabStatus;

describe("home model", () => {
  it("greets by the hour", () => {
    expect(greetingKey(8)).toBe("home.greeting.morning");
    expect(greetingKey(12)).toBe("home.greeting.afternoon");
    expect(greetingKey(18)).toBe("home.greeting.evening");
  });

  it("knows Docker's readiness only once the machine is checked", () => {
    expect(dockerReadiness(null)).toBeNull();
    expect(dockerReadiness({ docker: { installed: true }, dockerRunning: false } as SystemReport)).toBe(false);
    expect(dockerReadiness({ docker: { installed: true }, dockerRunning: true } as SystemReport)).toBe(true);
  });

  it("picks the line under the greeting", () => {
    expect(heroStatus(null, 3)).toBe("checking");
    expect(heroStatus(null, 3, true)).toBe("checkFailed");
    expect(heroStatus(true, 0, true)).toBe("ready");
    expect(heroStatus(false, 3)).toBe("setUpDocker");
    expect(heroStatus(true, 2)).toBe("running");
    expect(heroStatus(true, 0)).toBe("ready");
  });

  it("counts the running labs' own containers only", () => {
    const labs = [lab("a"), lab("b"), lab("vm", "VM"), lab("off")];
    const statuses = { a: status(true), b: status(true, { place: "cloud" }), vm: status(true), off: status(false) };
    const running = runningLabs(labs, statuses);
    expect(running.map((l) => l.id)).toEqual(["a", "b", "vm"]);
    // b runs in a cloud account and vm is a VM lab: only a's two containers are here.
    expect(labContainerCount(running, statuses)).toBe(2);
  });

  it("lists the labs launched last, newest first, not running, at most three", () => {
    const labs = ["a", "b", "c", "d", "e"].map((id) => lab(id));
    const runs: Record<string, number> = { a: 1, b: 5, c: 3, d: 4, e: 2 };
    const out = recentLabs(labs, { b: status(true) }, (id) => runs[id] ?? null);
    expect(out.map((r) => r.lab.id)).toEqual(["d", "c", "e"]);
    expect(recentLabs(labs, {}, () => null)).toEqual([]);
  });
});
