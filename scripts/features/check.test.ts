import { describe, expect, it } from "vitest";

import { problems, refProblem, splitRef } from "./check.ts";
import { factsOf, feature, registryOf, rigOf } from "./test-kit.ts";

const rig = rigOf([
  ["vm", "debian-13", "pass", "golden:libvirt"],
  ["vm", "macos-arm64", "notrun", "host:qemu"],
]);

const FILES = {
  "src-tauri/src/labs.rs": "fn helper() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a_lab_starts() {}\n}\n",
  "src-tauri/src/plain.rs": "fn not_a_test() {}\n",
  "src/features/labs/lab-store.test.ts": `it("keeps the last run", () => {});`,
  "src/features/labs/lab-store.ts": "export {};",
  "tests/web/labs.spec.ts": `test("filters narrow the list", async () => {});`,
};
const facts = factsOf(FILES, {
  commands: ["lab_launch", "lab_stop"],
  pages: ["tab:labs", "route:/shell"],
  scenarios: ["vm-lab", "setup"],
  desktopCases: new Map([
    ["lab-form-vm", "lab-form"],
    ["smoke", "smoke"],
  ]),
  desktopChecks: new Map([
    ["lab-form", new Set(["running"])],
    ["smoke", new Set(["smoke"])],
  ]),
  setupItems: ["docker", "plugin:<name>"],
});

describe("splitRef", () => {
  it("splits a file from what to find in it", () => {
    expect(splitRef("a.rs > b_c")).toEqual({ file: "a.rs", needle: "b_c" });
    expect(splitRef("a.test.ts")).toEqual({ file: "a.test.ts", needle: null });
    expect(splitRef("x.spec.ts > a > b")).toEqual({ file: "x.spec.ts", needle: "a > b" });
  });
});

describe("refProblem", () => {
  it("resolves a unit test: a test file, a Rust test by name, a TypeScript one by its words", () => {
    expect(refProblem("unit", "src-tauri/src/labs.rs", facts)).toBeNull();
    expect(refProblem("unit", "src-tauri/src/labs.rs > a_lab_starts", facts)).toBeNull();
    expect(refProblem("unit", "src/features/labs/lab-store.test.ts > keeps the last run", facts)).toBeNull();
    expect(refProblem("unit", "src-tauri/src/labs.rs > a_lab", facts)).toBe("src-tauri/src/labs.rs has no fn a_lab");
    expect(refProblem("unit", "src/features/labs/lab-store.test.ts > forgets", facts)).toBe('src/features/labs/lab-store.test.ts has no "forgets"');
    expect(refProblem("unit", "src-tauri/src/plain.rs", facts)).toBe("src-tauri/src/plain.rs is not a unit test file");
    expect(refProblem("unit", "src/features/labs/lab-store.ts", facts)).toBe("src/features/labs/lab-store.ts is not a unit test file");
    expect(refProblem("unit", "src/gone.test.ts", facts)).toBe("no file src/gone.test.ts");
  });

  it("resolves a web test in tests/web by its title's words", () => {
    expect(refProblem("web", "labs.spec.ts > filters narrow", facts)).toBeNull();
    expect(refProblem("web", "labs.spec.ts > sorts", facts)).toBe('labs.spec.ts has no "sorts"');
    expect(refProblem("web", "shell.spec.ts", facts)).toBe("no file tests/web/shell.spec.ts");
  });

  it("resolves a desktop case, and a check of its spec", () => {
    expect(refProblem("desktop", "lab-form-vm", facts)).toBeNull();
    expect(refProblem("desktop", "lab-form-vm/running", facts)).toBeNull();
    expect(refProblem("desktop", "lab-form-vm/engine", facts)).toBe("spec lab-form has no check engine");
    expect(refProblem("desktop", "lab-form", facts)).toMatch(/^no desktop case lab-form/);
  });

  it("resolves an integration scenario the runner lists, or an opt-in Rust test by name", () => {
    expect(refProblem("integration", "vm-lab", facts)).toBeNull();
    expect(refProblem("integration", "src-tauri/src/plain.rs > not_a_test", facts)).toBeNull();
    expect(refProblem("integration", "vm-labs", facts)).toMatch(/^no integration scenario vm-labs/);
    expect(refProblem("integration", "src/features/labs/lab-store.test.ts > keeps", facts)).toMatch(/is not a Rust file$/);
  });

  it("resolves a package check by its file", () => {
    expect(refProblem("package", "tests/web/labs.spec.ts", facts)).toBeNull();
    expect(refProblem("package", ".github/workflows/package-smoke.yml", facts)).toBe("no file .github/workflows/package-smoke.yml");
  });

  it("resolves a setup item, a pattern's name taking any value", () => {
    expect(refProblem("setup", "plugin:vagrant-qemu", facts)).toBeNull();
    expect(refProblem("setup", "wsl", facts)).toMatch(/^no setup item wsl/);
  });
});

describe("problems", () => {
  const complete = feature({
    commands: ["lab_launch", "lab_stop"],
    pages: ["tab:labs", "route:/shell"],
    tests: { integration: ["vm-lab"], desktop: ["lab-form-vm/running", "smoke"], setup: ["docker"] },
    rig: ["vm"],
  });

  it("is empty when every test exists and everything belongs to a feature", () => {
    expect(problems(registryOf([complete]), facts, rig)).toEqual([]);
  });

  it("names each test that doesn't exist and each rig column, command or page that doesn't", () => {
    const f = feature({
      ...complete,
      tests: { ...complete.tests, unit: ["src/gone.test.ts"] },
      rig: ["vm@qemu", "vm@golden:libvirt", "mars", "vm@hyperv"],
      commands: [...complete.commands, "lab_fly"],
    });
    expect(problems(registryOf([f]), facts, rig)).toEqual([
      "labs.a: unit `src/gone.test.ts`: no file src/gone.test.ts",
      "labs.a: rig `mars`: no rig column mars (known: vm)",
      "labs.a: rig `vm@hyperv`: no rig row readied by hyperv has a vm cell",
      "labs.a: no command lab_fly (src-tauri/src/commands.rs)",
    ]);
  });

  it("names each command, page, scenario and desktop case no feature claims", () => {
    expect(problems(registryOf([feature()]), facts, rig)).toEqual([
      "command lab_launch belongs to no feature: add it to one in tests/features.yml",
      "command lab_stop belongs to no feature: add it to one in tests/features.yml",
      "page tab:labs belongs to no feature: add it to one in tests/features.yml",
      "page route:/shell belongs to no feature: add it to one in tests/features.yml",
      "integration scenario vm-lab belongs to no feature: add it to one in tests/features.yml",
      "integration scenario setup belongs to no feature: add it to one in tests/features.yml",
      "desktop case lab-form-vm belongs to no feature: add it to one in tests/features.yml",
      "desktop case smoke belongs to no feature: add it to one in tests/features.yml",
    ]);
  });

  it("gives a command one home, while a page may hold several features", () => {
    const other = feature({ id: "labs.b", commands: ["lab_stop"], pages: ["tab:labs"] });
    expect(problems(registryOf([complete, other]), facts, rig)).toEqual(["command lab_stop is the home of one feature, not labs.a, labs.b"]);
  });

  it("names a known gap's feature that isn't one", () => {
    expect(problems(registryOf([complete], [{ id: "g", what: "G.", features: ["labs.zz"], status: "" }]), facts, rig)).toEqual([
      "known gap g: no feature labs.zz",
    ]);
  });
});
