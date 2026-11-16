import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

// CI runs the check and nothing else (CONTRIBUTING.md, "CI"): ci.yml its short list, gate.yml
// every step but the native layer, each job a few steps with ONLY. These hold the workflows to
// the check, so a step added to it is a step CI runs.

const ROOT = join(import.meta.dirname, "../..");
const read = (path: string) => readFileSync(join(ROOT, path), "utf8");
const CHECK = read("scripts/release/release-check.sh");

/** The check's steps, as release-check.sh's code names them (`step`, `nested` or `skip <name>`). */
function steps(): string[] {
  const code = CHECK.split("\n").filter((line) => !line.trimStart().startsWith("#"));
  const names = code.flatMap((line) => [...line.matchAll(/(?:^|[;\s(])(?:step|nested|skip) ([a-z][a-z0-9-]*)/g)].map((m) => m[1]));
  return [...new Set(names)];
}

/** The steps each `ONLY:` line of a workflow names, in order. */
function onlyLists(workflow: string): string[][] {
  return [...read(workflow).matchAll(/^\s+ONLY: (.+)$/gm)].map((m) => m[1].trim().split(/\s+/));
}

/** The short list release-check.sh runs for FAST=1. */
function fastSteps(): string[] {
  const line = CHECK.match(/^FAST_STEPS="([^"]+)"$/m);
  if (!line) throw new Error("release-check.sh has no FAST_STEPS line");
  return line[1].split(/\s+/);
}

/** The steps CI can't run, which the workspace's rig machines do. */
const NOT_IN_CI = ["e2e-native"];

describe("the workflows run the check", () => {
  it("finds the check's steps", () => {
    expect(steps()).toEqual(expect.arrayContaining(["prereqs", "clippy", "unit-rust", "coverage-rust", "hooks", "e2e-mock", "e2e-native"]));
  });

  it("the gate runs every step but the native layer, each a step the check has", () => {
    const known = steps();
    const ran = onlyLists(".github/workflows/gate.yml").flat();
    for (const step of ran) expect(known, `gate.yml runs "${step}", which the check doesn't have`).toContain(step);
    const missing = known.filter((s) => !ran.includes(s) && !NOT_IN_CI.includes(s));
    expect(missing, "steps of the check the gate never runs").toEqual([]);
  });

  it("every gate job checks its prerequisites and installs first", () => {
    for (const only of onlyLists(".github/workflows/gate.yml")) expect(only.slice(0, 2)).toEqual(["prereqs", "install"]);
  });

  it("ci runs the short list, which is made of the check's steps", () => {
    const ci = read(".github/workflows/ci.yml");
    expect(ci).toMatch(/^\s+FAST: "1"$/m);
    expect(ci).toMatch(/^\s+- run: scripts\/release\/release-check\.sh$/m);
    const known = steps();
    for (const step of fastSteps()) expect(known, `FAST_STEPS names "${step}"`).toContain(step);
  });

  it("no workflow runs a step's command on its own", () => {
    // The check's commands belong to it: a workflow running one besides it would drift from it.
    for (const workflow of ["ci.yml", "gate.yml"]) {
      const text = read(`.github/workflows/${workflow}`);
      for (const command of ["cargo test", "cargo nextest", "cargo clippy", "vitest run", "pnpm lint", "pnpm test:web", "cargo llvm-cov"]) {
        expect(text, `${workflow} runs \`${command}\` itself`).not.toContain(`run: ${command}`);
      }
    }
  });
});
