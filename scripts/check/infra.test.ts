import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

// scripts/check/infra.sh, the pins check lint-infra.yml and the release check share, run for real.

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** A repository holding the script and one workflow whose step `uses` this; the check's verdict. */
function pins(uses: string) {
  const root = mkdtempSync(join(tmpdir(), "check-infra-"));
  dirs.push(root);
  mkdirSync(join(root, "scripts/check"), { recursive: true });
  mkdirSync(join(root, ".github/workflows"), { recursive: true });
  mkdirSync(join(root, ".github/actions/setup"), { recursive: true });
  copyFileSync(join(import.meta.dirname, "infra.sh"), join(root, "scripts/check/infra.sh"));
  writeFileSync(join(root, ".github/workflows/ci.yml"), `jobs:\n  a:\n    steps:\n      - uses: ${uses}\n      - uses: ./.github/actions/setup\n`);
  writeFileSync(join(root, ".github/actions/setup/action.yml"), "runs:\n  steps: []\n");
  const r = spawnSync("bash", [join(root, "scripts/check/infra.sh"), "pins"], { encoding: "utf8" });
  return { status: r.status, out: r.stdout + r.stderr };
}

describe("check/infra.sh pins", () => {
  it("passes an action pinned to a commit SHA with its version, a local one, and fails a tag", () => {
    expect(pins("actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0").status).toBe(0);
    const tag = pins("actions/checkout@v4");
    expect(tag.status).toBe(1);
    expect(tag.out).toContain("uses: actions/checkout@v4");
    // A SHA without the version comment isn't enough: updates read the version there.
    expect(pins("actions/checkout@11d5960a326750d5838078e36cf38b85af677262").status).toBe(1);
  });

  it("checks this repository's workflows", () => {
    const r = spawnSync("bash", [join(import.meta.dirname, "infra.sh"), "pins"], { encoding: "utf8" });
    expect(r.status).toBe(0);
  });
});
