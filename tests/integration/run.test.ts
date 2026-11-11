import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
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
  const clean = {
    ...process.env,
    ONLY: "",
    SKIP: "",
    STRICT: "",
    SUITE_NESTED: "",
    CYBERCTF_TEST_ISOLOOM_DIR: dir,
    CYBERCTF_TEST_INTEGRATION_LOGS: dir,
    ...env,
  };
  const r = spawnSync("bash", [RUNNER, ...args], { env: { ...clean, PATH: `${dir}:${process.env.PATH ?? ""}` }, encoding: "utf8" });
  return { status: r.status, out: r.stdout + r.stderr };
}

/** The verdict a summary line gives `step`. */
const verdict = (out: string, step: string) => new RegExp(`^ {3}${step} +(ok|FAIL|skip) `, "m").exec(out)?.[1];

describe("an integration scenario's verdict", () => {
  /** The `docker-lab-hello-stack` scenario alone, with a `cargo` that answers `script`. */
  const scenario = (script: string) => verdict(runner(script, { ONLY: "docker-lab-hello-stack" }).out, "docker-lab-hello-stack");

  it("is ok when the test passed, skip when it returned for a knob not set, FAIL when it failed", () => {
    expect(scenario(`echo "test result: ok. 1 passed; 0 failed"`)).toBe("ok");
    expect(scenario(`echo "integration test skipped, not set: CYBERCTF_TEST_HELLO_LAB"; echo "test result: ok. 1 passed; 0 failed"`)).toBe("skip");
    expect(scenario(`echo "test result: FAILED. 0 passed; 1 failed"; exit 101`)).toBe("FAIL");
    // A filter that matched no test ran nothing: not a pass.
    expect(scenario(`echo "test result: ok. 0 passed; 0 failed"`)).toBe("FAIL");
  });

  it("reads a knob by its old name while the test rig sets that one", () => {
    // ISOLOOM for CYBERCTF_TEST_ISOLOOM_DIR: the runner finds the checkout by it.
    const isoloom = folder();
    const r = runner(`echo "test result: ok. 1 passed; 0 failed"`, { CYBERCTF_TEST_ISOLOOM_DIR: "", ISOLOOM: isoloom, ONLY: "docker-lab-hello-stack" });
    expect(verdict(r.out, "docker-lab-hello-stack")).toBe("ok");
    expect(runner("", { CYBERCTF_TEST_ISOLOOM_DIR: "/nonexistent" }).status).toBe(2);
  });
});

/** The Rust of a scenario file and its folder (`attack_box.rs`, `attack_box/…`), or of the whole crate. */
function rust(path: string): string {
  if (!existsSync(path)) return "";
  if (statSync(path).isDirectory())
    return readdirSync(path)
      .map((f) => rust(join(path, f)))
      .join("\n");
  return path.endsWith(".rs") ? readFileSync(path, "utf8") : "";
}
const SOURCES = join(import.meta.dirname, "../../src-tauri/src");
const SCENARIOS = join(SOURCES, "integration_tests");

interface Listed {
  suite: string;
  case: string;
  tier: string;
  form: string;
  test: string;
  knobs: string[];
}

/** The scenario file a listed test is in: `integration_tests::<file>::<test>`. */
const fileOf = (c: Listed) => c.test.split("::")[1] ?? "";

/** What `--list` prints, parsed. */
function listed(env: Record<string, string> = {}): Listed[] {
  const r = runner("exit 99", env, ["--list"]);
  expect(r.status).toBe(0);
  return r.out
    .trim()
    .split("\n")
    .map((l) => JSON.parse(l) as Listed);
}

/** Knobs a scenario file reads that no listed case does, each with why. */
const NOT_LISTED: Record<string, string> = {
  // docker_lab::supplier_portal_runs_end_to_end, run by labs.yml, which checks the lab out.
  CYBERCTF_TEST_INVOICE_LAB: "labs.yml",
  // The setup's report in its old format, while the test rig reads it.
  CYBERCTF_TEST_SETUP_REPORT: "the rig",
  // What a killed-start scenario hands the child process it kills, not a knob.
  CYBERCTF_TEST_RESILIENCE_CHILD: "resilience.rs",
};

describe("--list", () => {
  it("prints a scenario a line, each a test of integration_tests that exists, and runs nothing", () => {
    const all = listed({ CYBERCTF_TEST_ISOLOOM_DIR: "/nonexistent" });
    expect(new Set(all.map((c) => c.case)).size).toBe(all.length);
    expect(all.find((c) => c.case === "vm-lab")).toMatchObject({ suite: "integration", tier: "local", form: "vm" });
    expect(all.find((c) => c.case === "setup")).toMatchObject({
      tier: "bare-machine",
      test: "integration_tests::setup::the_guided_setup_readies_this_machine",
    });
    expect(new Set(all.map((c) => c.tier))).toEqual(new Set(["local", "aws", "server", "bare-machine"]));
    for (const c of all) {
      const [, file = "", name = ""] = c.test.split("::");
      expect(rust(join(SCENARIOS, `${file}.rs`)), c.test).toMatch(new RegExp(`async fn ${name}\\(`));
    }
  });

  it("names a scenario after its file", () => {
    for (const c of listed()) {
      const file = fileOf(c);
      expect(c.case === "setup" || c.case.startsWith(file.replaceAll("_", "-")), `${c.case}: ${c.test}`).toBe(true);
    }
  });

  it("lists every knob each scenario file reads, and only knobs the code reads", () => {
    const all = listed();
    const crate = rust(SOURCES);
    for (const file of new Set(all.map(fileOf))) {
      const reads = new Set(all.filter((c) => fileOf(c) === file).flatMap((c) => c.knobs));
      const code = rust(join(SCENARIOS, `${file}.rs`)) + rust(join(SCENARIOS, file));
      const used = [...code.matchAll(/"(CYBERCTF_TEST_[A-Z0-9_]+)"/g)].map((m) => m[1]);
      expect(
        used.filter((k) => !reads.has(k) && !(k in NOT_LISTED)),
        file,
      ).toEqual([]);
    }
    for (const knob of new Set(all.flatMap((c) => c.knobs))) expect(crate, knob).toContain(`"${knob}"`);
  });
});

describe("--results", () => {
  it("appends a JSON record per scenario: its form, provider, verdict, why, time and log", () => {
    const out = join(folder(), "results.jsonl");
    writeFileSync(out, "");
    const failing = `echo 'FAILED: the lab "solo" answers'; echo "test result: FAILED. 0 passed; 1 failed"; exit 101`;
    runner(failing, { ONLY: "vm-lab replace-docker", CYBERCTF_TEST_VM_PROVIDER: "libvirt" }, ["--results", out]);
    runner(`echo "test result: ok. 1 passed; 0 failed"`, { ONLY: "launch-commands" }, ["--results", out]);
    const records = readFileSync(out, "utf8")
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as Record<string, unknown>);
    expect(records.map((r) => [r.case, r.result, r.form, r.provider, r.reason])).toEqual([
      ["vm-lab", "fail", "vm", "libvirt", 'FAILED: the lab "solo" answers'],
      ["replace-docker", "fail", "docker", "", 'FAILED: the lab "solo" answers'],
      ["launch-commands", "pass", "docker", "", ""],
    ]);
    for (const r of records) {
      expect(Object.keys(r)).toEqual(["suite", "case", "provider", "form", "result", "reason", "seconds", "log"]);
      expect(r.suite).toBe("integration");
      expect(r.log).toMatch(new RegExp(`/${String(r.case)}\\.log$`));
      expect(typeof r.seconds).toBe("number");
    }
  });

  it("records a scenario the launcher refuses on its provider as skipped, the launcher's words its reason", () => {
    // The UTM pass: a lab whose VM is on a network can't run on UTM (support::refused_by_launcher).
    const out = join(folder(), "results.jsonl");
    const said = "This lab can't run on UTM yet: `web` is on a network, and vagrant_utm gives a VM no private network.";
    const cargo = `echo "integration test skipped, refused by the launcher: ${said.replaceAll("`", "\\`")}"; echo "test result: ok. 1 passed; 0 failed"`;
    runner(cargo, { ONLY: "vm-lab", CYBERCTF_TEST_VM_PROVIDER: "utm" }, ["--results", out]);
    const record = JSON.parse(readFileSync(out, "utf8")) as Record<string, unknown>;
    expect(record).toMatchObject({ case: "vm-lab", result: "skip", form: "vm", provider: "utm", reason: said });
    // A start the launcher refuses later, on a provider it takes, is the scenario's failure.
    const failed = join(folder(), "results.jsonl");
    runner(`echo "FAILED: it starts: This lab can't run on UTM yet"; echo "test result: FAILED. 0 passed; 1 failed"; exit 101`, { ONLY: "vm-lab" }, [
      "--results",
      failed,
    ]);
    expect(JSON.parse(readFileSync(failed, "utf8"))).toMatchObject({ case: "vm-lab", result: "fail" });
  });

  it("records a scenario its tier can't run here as skipped, with why", () => {
    const out = join(folder(), "results.jsonl");
    const env = { CYBERCTF_TEST_INTEGRATION_TIERS: "server", CYBERCTF_TEST_ESXI_HOST: "", CYBERCTF_TEST_ESXI_PASSWORD: "", ONLY: "server-lab-esxi" };
    runner("exit 99", env, ["--results", out]);
    const record = JSON.parse(readFileSync(out, "utf8")) as Record<string, unknown>;
    expect(record).toMatchObject({
      case: "server-lab-esxi",
      result: "skip",
      form: "vm",
      provider: "esxi",
      reason: expect.stringContaining("CYBERCTF_TEST_ESXI_HOST") as unknown,
    });
  });
});
