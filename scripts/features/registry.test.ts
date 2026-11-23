import { describe, expect, it } from "vitest";

import { features, parseRegistry } from "./registry.ts";

const one = (feature: string) => `areas:\n  - id: labs\n    title: Labs\n    features:\n${feature}`;

describe("parseRegistry", () => {
  it("reads an area's features with their defaults: every platform, no test, not on the host", () => {
    const { registry, problems } = parseRegistry(
      one("      - id: labs.search\n        what: Search the catalogue.\n        tests:\n          web: [labs.spec.ts > filters]\n"),
    );
    expect(problems).toEqual([]);
    const [f] = features(registry);
    expect(f).toMatchObject({ id: "labs.search", area: "labs", platforms: ["linux", "macos", "windows"], host: false, note: "" });
    expect(f.tests).toEqual({ unit: [], web: ["labs.spec.ts > filters"], desktop: [], integration: [], setup: [], package: [] });
  });

  it("keeps the platforms in their order and refuses one it doesn't know", () => {
    const { registry, problems } = parseRegistry(one("      - id: labs.a\n        what: A.\n        platforms: [windows, linux, beos]\n"));
    expect(features(registry)[0].platforms).toEqual(["linux", "windows"]);
    expect(problems).toEqual(["labs.a: unknown platforms beos (known: linux, macos, windows)"]);
  });

  it("refuses an unknown key, at the feature, its tests or its gaps: a typo would drop a test silently", () => {
    const { problems } = parseRegistry(
      one("      - id: labs.a\n        what: A.\n        test: {}\n        tests:\n          e2e: [x]\n        gaps:\n          unti: y\n"),
    );
    expect(problems).toEqual([
      expect.stringContaining("labs.a: unknown key `test`"),
      expect.stringContaining("labs.a tests: unknown key `e2e`"),
      expect.stringContaining("labs.a gaps: unknown key `unti`"),
    ]);
  });

  it("wants an id in its area, kebab-case, and a description", () => {
    const { problems } = parseRegistry(one("      - id: machine.Check_Up\n        host: yes\n"));
    expect(problems).toEqual([
      "machine.Check_Up: an id is `labs.<kebab-case-name>`",
      "machine.Check_Up: `host` must be true or false",
      "machine.Check_Up: `what` must be a non-empty string",
    ]);
  });

  it("refuses a feature listed twice and a list that isn't one", () => {
    const { problems } = parseRegistry(one("      - id: labs.a\n        what: A.\n        rig: vm\n      - id: labs.a\n        what: B.\n"));
    expect(problems).toEqual(["labs.a: `rig` must be a list of strings", "labs.a: listed twice"]);
  });

  it("reads the known gaps", () => {
    const { registry, problems } = parseRegistry(
      `${one("      - id: labs.a\n        what: A.\n")}known_gaps:\n  - id: g\n    what: Gone.\n    features: [labs.a]\n`,
    );
    expect(problems).toEqual([]);
    expect(registry.knownGaps).toEqual([{ id: "g", what: "Gone.", features: ["labs.a"], status: "" }]);
  });

  it("says when the text isn't YAML, or not the registry's shape", () => {
    expect(parseRegistry("areas: [").problems[0]).toMatch(/^not YAML/);
    expect(parseRegistry("- a").problems).toEqual(["the registry is a map with `areas` and `known_gaps`"]);
    expect(parseRegistry("areas:\n  - id: x\n    title: X\n").problems).toEqual(["x: an area lists its features"]);
  });
});
