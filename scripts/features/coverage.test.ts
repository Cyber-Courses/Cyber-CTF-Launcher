import { describe, expect, it } from "vitest";

import { biggestGaps, level, levelCounts, mark, platformsWithPasses, rigPassed } from "./coverage.ts";
import { feature, registryOf, rigOf } from "./test-kit.ts";

const UNIT = ["src/a.test.ts"];
const rig = rigOf([
  ["vm", "debian-13", "pass", "golden:libvirt"],
  ["vm", "windows-11", "fail", "golden:hyperv"],
  ["vm", "macos-arm64", "notrun", "host:qemu"],
  ["container", "macos-arm64", "pass", "host"],
]);

describe("mark", () => {
  it("is none without tests, partial with a gap said, covered otherwise", () => {
    expect(mark(feature(), "unit")).toBe("none");
    expect(mark(feature({ tests: { unit: UNIT } }), "unit")).toBe("covered");
    expect(mark(feature({ tests: { unit: UNIT }, gaps: { unit: "renders only" } }), "unit")).toBe("partial");
    expect(mark(feature({ gaps: { web: "no spec" } }), "web")).toBe("none");
  });
});

describe("level", () => {
  it("is Unchecked with no test in any tier, even when the gaps say why", () => {
    expect(level(feature({ gaps: { unit: "nothing" } }), rig)).toBe("Unchecked");
  });

  it("is Strong for the app's own behaviour with unit tests and any end-to-end tier covering it", () => {
    expect(level(feature({ tests: { unit: UNIT, web: ["x.spec.ts"] } }), rig)).toBe("Strong");
    expect(level(feature({ tests: { unit: UNIT, web: ["x.spec.ts"] }, gaps: { web: "renders only" } }), rig)).toBe("Partial");
    expect(level(feature({ tests: { web: ["x.spec.ts"] } }), rig)).toBe("Partial");
  });

  it("wants a real-host tier for what depends on the machine: the dev mock proves nothing there", () => {
    const host = { host: true, tests: { unit: UNIT, web: ["x.spec.ts"] } };
    expect(level(feature(host), rig)).toBe("Partial");
    expect(level(feature({ ...host, tests: { ...host.tests, integration: ["vm-lab"] } }), rig)).toBe("Strong");
    expect(level(feature({ ...host, tests: { ...host.tests, integration: ["vm-lab"] }, gaps: { integration: "Linux only" } }), rig)).toBe("Partial");
  });

  it("wants the rig cells a feature names to have passed somewhere: a real-host test never run is on paper", () => {
    const vm = { host: true, tests: { unit: UNIT, integration: ["vm-lab"] } };
    expect(level(feature({ ...vm, rig: ["vm@libvirt"] }), rig)).toBe("Strong");
    expect(level(feature({ ...vm, rig: ["vm@qemu", "vm@hyperv"] }), rig)).toBe("Partial");
  });

  it("is counted per level", () => {
    expect(levelCounts([feature(), feature({ tests: { web: ["x"] } }), feature()], rig)).toEqual({ Strong: 0, Partial: 1, Unchecked: 2 });
  });
});

describe("the rig", () => {
  it("says where a feature's cells passed, of its platforms, a row picked by what readied it", () => {
    expect(rigPassed(feature({ rig: ["vm"] }), rig)).toEqual(["linux"]);
    expect(rigPassed(feature({ rig: ["vm", "container"], platforms: ["macos", "windows"] }), rig)).toEqual(["macos"]);
    expect(rigPassed(feature({ rig: ["vm@qemu"] }), rig)).toEqual([]);
    expect(platformsWithPasses(rig)).toEqual(["linux", "macos"]);
  });
});

describe("biggestGaps", () => {
  it("puts the owner's known gaps first, then the untested, the rig's reach, no real host, never run on the rig", () => {
    const list = [
      feature({ id: "labs.ui", what: "Pick a theme." }),
      feature({ id: "labs.host", what: "Run a VM", host: true }),
      feature({ id: "labs.mock", what: "Start.", host: true, tests: { unit: UNIT, web: ["x"] }, rig: ["vm@libvirt"] }),
      feature({ id: "labs.paper", what: "Pause", host: true, tests: { unit: UNIT, integration: ["vm-lab"] }, rig: ["vm@qemu"] }),
      feature({ id: "labs.fine", what: "Stop", host: true, tests: { unit: UNIT, integration: ["vm-lab"] }, platforms: ["linux"], rig: ["vm"] }),
    ];
    const gaps = biggestGaps(registryOf(list, [{ id: "k", what: "Known", features: ["labs.ui"], status: "planned" }]), rig, 10);
    expect(gaps.map((g) => g.title)).toEqual([
      "Known",
      "labs.host: no test",
      "labs.ui: no test",
      "2 features that depend on the machine have never passed on the rig on one of their platforms",
      "labs.mock: no real-host test",
      "labs.paper: its real-host tests never passed on the rig",
    ]);
    expect(gaps[0]).toEqual({ title: "Known", detail: "planned", features: ["labs.ui"] });
    expect(gaps[3]).toMatchObject({
      detail: "the rig's matrix (generated 2026-01-01) has passing cells on linux, macos only",
      features: ["labs.mock", "labs.paper"],
    });
    expect(gaps[4]).toEqual({ title: "labs.mock: no real-host test", detail: "no desktop, integration, setup or package test", features: [] });
    expect(gaps[5].detail).toBe("Rig cells: vm@qemu.");
    expect(biggestGaps(registryOf(list), rig, 1)).toHaveLength(1);
  });
});
