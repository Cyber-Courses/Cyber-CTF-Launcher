import { describe, expect, it } from "vitest";

import { formatted, parseArgs, snapshotJson } from "./main.ts";
import { render } from "./render.ts";
import { feature, registryOf, rigOf } from "./test-kit.ts";

const rig = rigOf([
  ["vm", "debian-13", "pass"],
  ["vm", "windows-11", "notrun"],
]);
const list = [
  feature({
    id: "labs.vm",
    what: "Run a VM lab | anywhere.",
    host: true,
    providers: ["qemu"],
    tests: { unit: ["a.rs > b"], integration: ["vm-lab"] },
    rig: ["vm"],
  }),
  feature({ id: "labs.theme", what: "Pick a theme.", platforms: ["macos"], tests: { web: ["s.spec.ts > t"] }, gaps: { web: "renders only" } }),
  feature({ id: "labs.none", what: "Do nothing.", note: "Planned." }),
];
const md = render(registryOf(list), rig);

describe("render", () => {
  it("counts the levels at the top and per area", () => {
    expect(md).toContain("**3 features: 1 Strong, 1 Partial, 1 Unchecked.**");
    expect(md).toContain("| [Labs](#labs) | 3 | 1 | 1 | 1 |");
  });

  it("gives a row a feature: its marks per tier, the rig per platform, its level; a pipe escaped", () => {
    expect(md).toContain("| `labs.vm` Run a VM lab \\| anywhere. | all | ✔ | ✖ | ✖ | ✔ | ✖ | ✖ | Linux ✅, macOS –, Windows · | **Strong** |");
    expect(md).toContain("| `labs.theme` Pick a theme. | macOS | ✖ | ◐ | ✖ | ✖ | ✖ | ✖ | – | **Partial** |");
  });

  it("says what a partial tier leaves out, and lists each feature's tests", () => {
    expect(md).toContain("- `labs.theme`: Web E2E ◐ renders only");
    expect(md).toContain("- `labs.none`: Planned.");
    expect(md).toContain("- `labs.vm` (qemu): Unit: `a.rs > b`. Integration: `vm-lab`. Rig: `vm`.");
    expect(md).toContain("- `labs.none`: no test.");
  });

  it("lists the biggest gaps, the untested first", () => {
    expect(md).toContain("## Biggest gaps\n\n1. `labs.none`: **no test**: Planned.\n");
  });

  it("is stable under Prettier, so the check compares like with like", async () => {
    const once = await formatted(md, "tests/FEATURES.md");
    expect(await formatted(once, "tests/FEATURES.md")).toBe(once);
  });
});

describe("snapshotJson", () => {
  it("writes a row a line, and reads back as it was", () => {
    const json = snapshotJson(rig);
    expect(json.split("\n").filter((l) => l.includes('"os"'))).toHaveLength(2);
    expect(JSON.parse(json)).toEqual(rig);
  });
});

describe("parseArgs", () => {
  it("takes --write and --matrix <file>, which writes too, and refuses anything else", () => {
    expect(parseArgs([])).toEqual({ write: false, matrix: null });
    expect(parseArgs(["--matrix", "../rig/MATRIX.md"])).toEqual({ write: true, matrix: "../rig/MATRIX.md" });
    expect(parseArgs(["--matrix"])).toEqual({ error: "unknown argument `--matrix` (see pnpm check:features --help)" });
    expect(parseArgs(["--wirte"])).toHaveProperty("error");
  });
});
