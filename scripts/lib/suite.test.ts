import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
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
  // Each inner runner is a script written just now, so the outer one runs it through bash, as
  // `run` does the outer: macOS checks a program new to it on its first run, and with a build
  // starting new programs by the hundred that check took seconds per script, past the test's 20 s.
  const inner = (body: string) => runner("inner.sh", `${body}\nsuite_end "inner"`);

  it("is recorded skip when it skipped any of its own steps, ok when it ran them all, FAIL when one failed", () => {
    const skipped = inner(`step a true\nskip b "no tool"`);
    const passed = inner("step a true");
    const failed = inner(`step a true\nstep b false\nskip c "no tool"`);
    const outer = runner("outer.sh", `nested skipped bash "${skipped}"\nnested passed bash "${passed}"\nnested failed bash "${failed}"\nsuite_end "outer"`);
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
    const outer = runner("outer.sh", `nested inner bash "${both}"\nstep other true\nsuite_end "outer"`);
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

describe("a runner's results", () => {
  /** A runner writing its records to a file of its own: the records, parsed. */
  function records(body: string, env: Record<string, string> = {}) {
    const path = runner("records.sh", `suite_results_to demo results.jsonl\n${body}\nsuite_end "records"`);
    const dir = mkdtempSync(join(tmpdir(), "suite-results-"));
    dirs.push(dir);
    const r = spawnSync("bash", [path], { cwd: dir, env: { ...process.env, ONLY: "", SKIP: "", STRICT: "", SUITE_NESTED: "", ...env }, encoding: "utf8" });
    const text = readFileSync(join(dir, "results.jsonl"), "utf8");
    return {
      status: r.status,
      lines: text
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l) as Record<string, unknown>),
    };
  }

  it("are a JSON line per verdict, the contract's fields in order, appended where the runner was started", () => {
    const { lines } = records(`step a true\nrecord b FAIL 7 "exit 1" vm libvirt /logs/b.log\nskip c "no tool" docker`);
    expect(lines).toEqual([
      { suite: "demo", case: "a", provider: "", form: "", result: "pass", reason: "", seconds: 0, log: "" },
      { suite: "demo", case: "b", provider: "libvirt", form: "vm", result: "fail", reason: "exit 1", seconds: 7, log: "/logs/b.log" },
      { suite: "demo", case: "c", provider: "", form: "docker", result: "skip", reason: "no tool", seconds: 0, log: "" },
    ]);
    expect(Object.keys(lines[0] ?? {})).toEqual(["suite", "case", "provider", "form", "result", "reason", "seconds", "log"]);
  });

  it("keep a reason's quotes, backslashes and tabs, and make it one line", () => {
    const { lines } = records(`record a FAIL 1 $'the "lab"\\\\ said:\\tno\\nmore\\r\\x1b[31m'`);
    expect(lines[0]?.reason).toBe('the "lab"\\ said:\tno more [31m');
  });

  it("give a step's checks a line each, named <step>/<check>, not counted again", () => {
    const log = `printf '%s\\n' 'ok: start (3s) it runs' 'FAILED: stop (9s) left a VM' 'skipped: spec (0s) whole' 'noise' > "$PWD/spec.log"`;
    const { status, lines } = records(`${log}\nrecord s ok 12 "" vm qemu "$PWD/spec.log"\nstep_checks s "$PWD/spec.log" vm qemu`);
    expect(lines.map((l) => [l.case, l.result, l.seconds, l.reason, l.form, l.provider])).toEqual([
      ["s", "pass", 12, "", "vm", "qemu"],
      ["s/start", "pass", 3, "", "vm", "qemu"],
      ["s/stop", "fail", 9, "left a VM", "vm", "qemu"],
    ]);
    // The failed check isn't counted: the spec passed.
    expect(status).toBe(0);
  });

  it("are written only when asked for", () => {
    const path = runner("quiet.sh", `step a true\nsuite_end "quiet"`);
    const dir = mkdtempSync(join(tmpdir(), "suite-quiet-"));
    dirs.push(dir);
    spawnSync("bash", [path], { cwd: dir, encoding: "utf8" });
    expect(readdirSync(dir)).toEqual([]);
  });
});

describe("the dev server a runner starts for the frontend", () => {
  it("gives up on a server that takes the request and never answers", async () => {
    // Takes the connection and says nothing, as a `next dev` lost in its own cache did.
    const silent = createServer(() => {});
    await new Promise<void>((resolve) => silent.listen(0, "127.0.0.1", resolve));
    const port = (silent.address() as { port: number }).port;
    try {
      const path = runner("probe.sh", `serves ${String(port)} && echo answered || echo "no answer"`);
      const start = Date.now();
      const r = spawnSync("bash", [path], { env: { ...process.env, SERVES_TIMEOUT: "1" }, encoding: "utf8", timeout: 15000 });
      expect(r.status, r.stdout + r.stderr).toBe(0);
      expect(r.stdout).toContain("no answer");
      expect(Date.now() - start).toBeLessThan(10000);
    } finally {
      silent.close();
    }
  });

  it.runIf(process.platform !== "win32")("is next dev in the frontend's folder on a dev cache of its own, the rest of .next kept", () => {
    const dir = mkdtempSync(join(tmpdir(), "suite-next-"));
    dirs.push(dir);
    const app = join(dir, "app");
    mkdirSync(join(app, ".next", "dev", "cache"), { recursive: true });
    writeFileSync(join(app, ".next", "dev", "cache", "tasks"), "a cache an older run left");
    writeFileSync(join(app, ".next", "BUILD_ID"), "a build's");
    // A pnpm that says where it ran, with what, and whether the dev cache was there.
    const bin = join(dir, "bin");
    mkdirSync(bin);
    writeFileSync(join(bin, "pnpm"), `#!/bin/sh\n{ echo "$PWD"; echo "$*"; [ -e .next/dev ] && echo cache || echo "no cache"; } > "${join(dir, "ran")}"\n`);
    chmodSync(join(bin, "pnpm"), 0o755);
    const path = runner("front.sh", `start_next_dev "${app}" 4321 "${join(dir, "next.log")}"\nwait "$NEXT_DEV_PGID"\necho "group $NEXT_DEV_PGID"`);
    const r = run(path, { PATH: `${bin}:${process.env.PATH ?? ""}` });
    expect(r.status, r.out).toBe(0);
    expect(r.out).toMatch(/group \d+/);
    expect(readFileSync(join(dir, "ran"), "utf8").split("\n")).toEqual([app, "exec next dev --hostname 127.0.0.1 --port 4321", "no cache", ""]);
    expect(existsSync(join(app, ".next", "dev"))).toBe(false);
    expect(readFileSync(join(app, ".next", "BUILD_ID"), "utf8")).toBe("a build's");
  });
});
