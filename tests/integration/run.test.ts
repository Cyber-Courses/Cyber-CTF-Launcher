import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

// tests/integration/run.sh, the integration runner, run for real in bash with a `cargo` that answers
// what each case needs: no lab is started.

const RUNNER = join(import.meta.dirname, "run.sh");
const dirs: string[] = [];

afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** A fresh folder, removed after the test. */
function folder(): string {
  const dir = mkdtempSync(join(tmpdir(), "integration-test-"));
  dirs.push(dir);
  return dir;
}

/** The runner with `args` under `env`, a `cargo` on its PATH that runs `cargo` (a shell script). */
function runner(cargo: string, env: Record<string, string>, args: string[] = []) {
  const dir = folder();
  writeFileSync(join(dir, "cargo"), `#!/bin/sh\n${cargo}\n`);
  chmodSync(join(dir, "cargo"), 0o755);
  const clean = { ...process.env, ONLY: "", SKIP: "", STRICT: "", SUITE_NESTED: "", ISOLOOM: dir, LOGS: dir, ...env };
  const r = spawnSync("bash", [RUNNER, ...args], { env: { ...clean, PATH: `${dir}:${process.env.PATH ?? ""}` }, encoding: "utf8" });
  return { status: r.status, out: r.stdout + r.stderr };
}

/** The verdict a summary line gives `step`. */
const verdict = (out: string, step: string) => new RegExp(`^ {3}${step} +(ok|FAIL|skip) `, "m").exec(out)?.[1];

describe("an integration scenario's verdict", () => {
  /** The `docker-lab` scenario alone, with a `cargo` that answers `script`. */
  const scenario = (script: string) => verdict(runner(script, { ONLY: "docker-lab" }).out, "docker-lab");

  it("is ok when the test passed, skip when it returned for a knob not set, FAIL when it failed", () => {
    expect(scenario(`echo "test result: ok. 1 passed; 0 failed"`)).toBe("ok");
    expect(scenario(`echo "integration test skipped, not set: CYBERCTF_TEST_HELLO_LAB"; echo "test result: ok. 1 passed; 0 failed"`)).toBe("skip");
    expect(scenario(`echo "test result: FAILED. 0 passed; 1 failed"; exit 101`)).toBe("FAIL");
    // A filter that matched no test ran nothing: not a pass.
    expect(scenario(`echo "test result: ok. 0 passed; 0 failed"`)).toBe("FAIL");
  });
});
