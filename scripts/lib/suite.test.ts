import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

// scripts/lib/suite.sh, the suite runners' shared verdicts, run for real in bash.

const SUITE = join(import.meta.dirname, "suite.sh");
const dirs: string[] = [];

afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** Writes `body` as an executable runner script sourcing suite.sh; its path. */
function runner(name: string, body: string): string {
  const dir = mkdtempSync(join(tmpdir(), "suite-test-"));
  dirs.push(dir);
  const path = join(dir, name);
  writeFileSync(path, `#!/usr/bin/env bash\n# The ${name} runner.\n#\n#   ${name} [--help]\nset -uo pipefail\n. "${SUITE}"\nsuite_help "$0" "$@"\n${body}\n`);
  chmodSync(path, 0o755);
  return path;
}

/** Runs `path` (with `args`) under `env`: its exit status and output. */
function run(path: string, env: Record<string, string> = {}, args: string[] = []) {
  const clean = { ...process.env, ONLY: "", SKIP: "", STRICT: "", SUITE_NESTED: "", ...env };
  const r = spawnSync("bash", [path, ...args], { env: clean, encoding: "utf8" });
  return { status: r.status, out: r.stdout + r.stderr };
}

/** The verdict a summary line gives `step`. */
const verdict = (out: string, step: string) => new RegExp(`^ {3}${step} +(ok|FAIL|skip) `, "m").exec(out)?.[1];

describe("a runner run as a step of another", () => {
  const inner = (body: string) => runner("inner.sh", `${body}\nsuite_end "inner"`);

  it("is recorded skip when it skipped any of its own steps, ok when it ran them all, FAIL when one failed", () => {
    const skipped = inner(`step a true\nskip b "no tool"`);
    const passed = inner("step a true");
    const failed = inner(`step a true\nstep b false\nskip c "no tool"`);
    const outer = runner("outer.sh", `nested skipped "${skipped}"\nnested passed "${passed}"\nnested failed "${failed}"\nsuite_end "outer"`);
    const { status, out } = run(outer);
    expect(verdict(out, "skipped")).toBe("skip");
    expect(verdict(out, "passed")).toBe("ok");
    expect(verdict(out, "failed")).toBe("FAIL");
    expect(status).toBe(1);
  });

  it("alone, exits 0 on a skip, as before, and 1 under STRICT", () => {
    const skipped = inner(`step a true\nskip b "no tool"`);
    expect(run(skipped).status).toBe(0);
    expect(run(skipped, { STRICT: "1" }).status).toBe(1);
  });

  it("runs all of its steps whatever ONLY and SKIP the outer runner was given", () => {
    const both = inner("step a true\nstep b true");
    const outer = runner("outer.sh", `nested inner "${both}"\nstep other true\nsuite_end "outer"`);
    const { out } = run(outer, { ONLY: "inner" });
    expect(out).toContain("==== a: true");
    expect(out).toContain("==== b: true");
    expect(verdict(out, "inner")).toBe("ok");
    expect(verdict(out, "other")).toBeUndefined();
  });
});

describe("a runner's steps", () => {
  const three = () => runner("three.sh", `step a true\nstep b true\nskip c "no tool"\nsuite_end "three"`);
  const ran = (out: string) => ["a", "b", "c"].filter((s) => verdict(out, s) !== undefined);

  it("runs only what ONLY names, all but what SKIP names", () => {
    expect(ran(run(three(), { ONLY: "a c" }).out)).toEqual(["a", "c"]);
    expect(ran(run(three(), { SKIP: "a" }).out)).toEqual(["b", "c"]);
    // A name is matched whole: "a" doesn't take "ab".
    const ab = runner("ab.sh", `step ab true\nstep a true\nsuite_end "ab"`);
    expect(verdict(run(ab, { ONLY: "a" }).out, "ab")).toBeUndefined();
  });

  it("fails on a failed step, and on a skipped one only under STRICT, and says how many", () => {
    const failing = runner("failing.sh", `step a false\nskip b "no tool"\nsuite_end "failing"`);
    const failed = run(failing);
    expect(failed.status).toBe(1);
    expect(verdict(failed.out, "a")).toBe("FAIL");
    expect(failed.out).toContain("1 failed, 1 skipped");
    expect(run(three()).status).toBe(0);
    expect(run(three(), { STRICT: "1" }).status).toBe(1);
  });

  it("prints its header comment on --help, without the comment marks, and runs nothing", () => {
    const { status, out } = run(three(), {}, ["--help"]);
    expect(status).toBe(0);
    expect(out).toBe("The three.sh runner.\n\n  three.sh [--help]\n");
  });
});

describe("an integration scenario's verdict", () => {
  /** scripts/integration-tests.sh's `docker-lab` alone, with a `cargo` that answers `script`. */
  function scenario(script: string) {
    const dir = mkdtempSync(join(tmpdir(), "scenario-test-"));
    dirs.push(dir);
    writeFileSync(join(dir, "cargo"), `#!/bin/sh\n${script}\n`);
    chmodSync(join(dir, "cargo"), 0o755);
    const path = `${dir}:${process.env.PATH ?? ""}`;
    const r = run(join(import.meta.dirname, "..", "integration-tests.sh"), { PATH: path, ISOLOOM: dir, LOGS: dir, ONLY: "docker-lab" });
    return verdict(r.out, "docker-lab");
  }

  it("is ok when the test passed, skip when it returned for a knob not set, FAIL when it failed", () => {
    expect(scenario(`echo "test result: ok. 1 passed; 0 failed"`)).toBe("ok");
    expect(scenario(`echo "integration test skipped, not set: CYBERCTF_TEST_HELLO_LAB"; echo "test result: ok. 1 passed; 0 failed"`)).toBe("skip");
    expect(scenario(`echo "test result: FAILED. 0 passed; 1 failed"; exit 101`)).toBe("FAIL");
    // A filter that matched no test ran nothing: not a pass.
    expect(scenario(`echo "test result: ok. 0 passed; 0 failed"`)).toBe("FAIL");
  });
});
