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
    CYBERCTF_TEST_INTEGRATION_TIMEOUT: "",
    CYBERCTF_TEST_VM_PROVIDER: "",
    CYBERCTF_TEST_DOCKER_VM_PROVIDER: "",
    CYBERCTF_TEST_ISOLOOM_DIR: dir,
    CYBERCTF_TEST_INTEGRATION_LOGS: dir,
    ...env,
  };
  const r = spawnSync("bash", [RUNNER, ...args], { env: { ...clean, PATH: `${dir}:${process.env.PATH ?? ""}` }, encoding: "utf8" });
  return { status: r.status, out: r.stdout + r.stderr, dir };
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

  it("is FAIL, saying so, when the test ran past its cap, cargo and what it ran killed", () => {
    // A cargo that would run for a minute, under a cap of 2 s (the runner looks every 2 s).
    const r = runner(`echo "FAILED: nothing yet"; sleep 60 & echo $! > "$CYBERCTF_TEST_INTEGRATION_LOGS/sleeper"; wait`, {
      ONLY: "docker-lab-hello-stack",
      CYBERCTF_TEST_INTEGRATION_TIMEOUT: "2",
    });
    expect(verdict(r.out, "docker-lab-hello-stack")).toBe("FAIL");
    expect(r.out).toMatch(/docker-lab-hello-stack +FAIL +\d+s {2}exceeded its 2s \(FAILED: nothing yet\)/);
    // The test binary's children went with it: nothing of the lab's start is left running.
    const sleeper = Number(readFileSync(join(r.dir, "sleeper"), "utf8").trim());
    expect(() => process.kill(sleeper, 0)).toThrow();
  });

  it("runs in a temp folder of its own that goes when it ends, passed, failed or killed past its cap, with what still runs from it", { timeout: 60000 }, () => {
    // The machine's temp folder, as the runner is given it: the scenario's test makes a lab's copy,
    // the runner's home and a test thread's folder in its own; one also leaves a VM running from it
    // that ignores the cap's TERM, as a daemonized QEMU outlives the test's process group.
    const machine = folder();
    const cargo = [
      'echo "$TMPDIR" >"$CYBERCTF_TEST_INTEGRATION_LOGS/temp"',
      'mkdir -p "$TMPDIR/cyberctf-box-1-home/runs" "$TMPDIR/cyberctf-vm-2" "$TMPDIR/cyberctf-test-home-AB12CD"',
      'case "$CASE" in',
      '  pass) echo "test result: ok. 1 passed; 0 failed" ;;',
      '  fail) echo "test result: FAILED. 0 passed; 1 failed"; exit 101 ;;',
      `  hang) (trap '' TERM; exec sh -c 'sleep 30; :' "$TMPDIR/cyberctf-vm-2/qemu") & echo $! >"$CYBERCTF_TEST_INTEGRATION_LOGS/vm"; sleep 30 ;;`,
      "esac",
    ].join("\n");
    const ours = () => readdirSync(machine).filter((n) => n.startsWith("cyberctf"));
    for (const [kase, result] of [
      ["pass", "ok"],
      ["fail", "FAIL"],
      ["hang", "FAIL"],
    ]) {
      const r = runner(cargo, { ONLY: "docker-lab-hello-stack", CASE: kase, TMPDIR: machine, CYBERCTF_TEST_INTEGRATION_TIMEOUT: "2" });
      expect(verdict(r.out, "docker-lab-hello-stack"), kase).toBe(result);
      const temp = readFileSync(join(r.dir, "temp"), "utf8").trim();
      expect(temp, kase).toMatch(/\/cyberctf-it\.[A-Za-z0-9]{6}$/);
      expect(existsSync(temp), `${kase}: ${temp} is left`).toBe(false);
      expect(ours(), kase).toEqual([]);
      // Its log names it: the leftover audit takes a folder a run's log names as the run's.
      expect(readFileSync(join(r.dir, "docker-lab-hello-stack.log"), "utf8"), kase).toContain(`suite: the scenario's temp folder was ${temp}`);
      if (kase === "hang") {
        const vm = Number(readFileSync(join(r.dir, "vm"), "utf8").trim());
        expect(() => process.kill(vm, 0), "the VM running from it is stopped").toThrow();
      }
    }
  });

  it("past its cap, ends its lab's VirtualBox VMs, which live outside its temp folder, before the folder goes", { timeout: 60000 }, () => {
    // Vagrant keeps a VirtualBox VM in ~/VirtualBox VMs and its id in the lab's .vagrant; a
    // VirtualBox stood in for by a VBoxManage that knows that one VM and records what it's asked.
    const machine = folder();
    const id = "3c32882b-cf96-4a01-a754-ca7386e37b49";
    const vbox = [
      "#!/bin/sh",
      'echo "$*" >>"$CYBERCTF_TEST_INTEGRATION_LOGS/vbox.calls"',
      `case "$1 $2" in "showvminfo ${id}") echo 'name="solo-web - attackbox"' ;; showvminfo*) exit 1 ;; esac`,
    ].join("\n");
    const cargo = [
      `printf '%s\\n' '${vbox.replaceAll("'", "'\\''")}' >"$(dirname "$0")/VBoxManage"`,
      'chmod +x "$(dirname "$0")/VBoxManage"',
      'mkdir -p "$TMPDIR/cyberctf-box-1/.vagrant/machines/attackbox/virtualbox" "$TMPDIR/cyberctf-box-1/.vagrant/machines/web/virtualbox"',
      `echo ${id} >"$TMPDIR/cyberctf-box-1/.vagrant/machines/attackbox/virtualbox/id"`,
      'echo 00000000-0000-0000-0000-000000000000 >"$TMPDIR/cyberctf-box-1/.vagrant/machines/web/virtualbox/id"',
      'case "$CASE" in hang) sleep 30 ;; pass) echo "test result: ok. 1 passed; 0 failed" ;; esac',
    ].join("\n");
    const capped = runner(cargo, { ONLY: "attack-box-vm", CASE: "hang", TMPDIR: machine, CYBERCTF_TEST_INTEGRATION_TIMEOUT: "2" });
    expect(verdict(capped.out, "attack-box-vm")).toBe("FAIL");
    const calls = readFileSync(join(capped.dir, "vbox.calls"), "utf8").split("\n").filter(Boolean);
    expect([...calls].sort()).toEqual(
      [
        `showvminfo ${id} --machinereadable`,
        `controlvm ${id} poweroff`,
        `unregistervm ${id} --delete`,
        "showvminfo 00000000-0000-0000-0000-000000000000 --machinereadable",
      ].sort(),
    );
    expect(calls.indexOf(`controlvm ${id} poweroff`), "powered off, then deleted").toBeLessThan(calls.indexOf(`unregistervm ${id} --delete`));
    expect(readFileSync(join(capped.dir, "attack-box-vm.log"), "utf8")).toContain(
      `suite: stopping the scenario's VirtualBox VM solo-web - attackbox (${id}): powered off and deleted`,
    );
    expect(readdirSync(machine).filter((n) => n.startsWith("cyberctf"))).toEqual([]);
    // A scenario that ended on its own cleaned up after itself, or the leftover audit says what it left.
    const ended = runner(cargo, { ONLY: "attack-box-vm", CASE: "pass", TMPDIR: machine, CYBERCTF_TEST_INTEGRATION_TIMEOUT: "20" });
    expect(verdict(ended.out, "attack-box-vm")).toBe("ok");
    expect(existsSync(join(ended.dir, "vbox.calls"))).toBe(false);
  });

  it("takes --only <scenario>@<provider>: that scenario alone, on that hypervisor", () => {
    const out = join(folder(), "results.jsonl");
    runner(`echo "test result: ok. 1 passed; 0 failed"`, {}, ["--only", "vm-lab@virtualbox", "--results", out]);
    runner(`echo "test result: ok. 1 passed; 0 failed"`, {}, ["--only", "attack-box-docker@docker", "--only", "docker-vm-lab@hyperv", "--results", out]);
    const records = readFileSync(out, "utf8")
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as Record<string, unknown>);
    expect(records.map((r) => [r.case, r.provider, r.result])).toEqual([
      ["vm-lab", "virtualbox", "pass"],
      ["attack-box-docker", "", "pass"],
      ["docker-vm-lab", "hyperv", "pass"],
    ]);
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

  it(
    "skips a provider's remaining scenarios once one failed on the platform itself, naming the failure; a lab's failure fails its scenario alone",
    { timeout: 180000 },
    () => {
      const out = join(folder(), "results.jsonl");
      // vm-lab fails with the launcher's own refusal of UTM: the VM scenarios after it on UTM are
      // skipped with that line, the docker ones (no provider) and those on another hypervisor run.
      const refusal = "FAILED: it starts: This lab can't run on UTM yet: `web` is on a network, and vagrant_utm gives a VM no private network.";
      const cargo = `case "$*" in *integration_tests::vm_lab::*) echo "${refusal.replaceAll("`", "\\`")}"; echo "test result: FAILED. 0 passed; 1 failed"; exit 101 ;; *) echo "test result: ok. 1 passed; 0 failed" ;; esac`;
      runner(
        cargo,
        {
          ONLY: "attack-box-vm docker-lab-hello-stack vm-lab per-machine replace-docker-vm replace-vm resilience-docker-port-taken resilience-vm-destroyed resilience-vm-killed-provisioning",
          CYBERCTF_TEST_VM_PROVIDER: "utm",
          CYBERCTF_TEST_DOCKER_VM_PROVIDER: "qemu",
        },
        ["--results", out],
      );
      const records = readFileSync(out, "utf8")
        .trim()
        .split("\n")
        .map((l) => JSON.parse(l) as Record<string, unknown>);
      const of = (name: string) => records.find((r) => r.case === name);
      expect(of("vm-lab")).toMatchObject({ result: "fail", provider: "utm", reason: refusal });
      for (const later of ["per-machine", "replace-vm", "resilience-vm-destroyed", "resilience-vm-killed-provisioning"])
        expect(of(later), later).toMatchObject({
          result: "skip",
          provider: "utm",
          reason: `utm failed on the platform earlier in this run: ${refusal}`,
          seconds: 0,
        });
      for (const other of ["attack-box-vm", "docker-lab-hello-stack", "replace-docker-vm", "resilience-docker-port-taken"])
        expect(of(other), other).toMatchObject({ result: "pass" });
      // Docker is a provider of its own: Docker down ends its scenarios.
      const down = join(folder(), "results.jsonl");
      const docker = `case "$*" in *hello_stack*) echo "thread 'main' panicked: Docker isn't running. Start your container engine"; exit 101 ;; *) echo "test result: ok. 1 passed; 0 failed" ;; esac`;
      runner(docker, { ONLY: "docker-lab-hello-stack docker-lab-segmented vm-lab", CYBERCTF_TEST_VM_PROVIDER: "libvirt" }, ["--results", down]);
      const after = readFileSync(down, "utf8")
        .trim()
        .split("\n")
        .map((l) => JSON.parse(l) as Record<string, unknown>);
      expect(after.map((r) => [r.case, r.result])).toEqual([
        ["docker-lab-hello-stack", "fail"],
        ["docker-lab-segmented", "skip"],
        ["vm-lab", "pass"],
      ]);
      expect(after[1]?.reason).toMatch(/^docker failed on the platform earlier in this run: .*Docker isn't running/);
      // A failure in the lab itself is that scenario's alone.
      const lab = join(folder(), "results.jsonl");
      const broken = `case "$*" in *integration_tests::vm_lab::*) echo "FAILED: the web machine answers: connection refused"; echo "test result: FAILED. 0 passed; 1 failed"; exit 101 ;; *) echo "test result: ok. 1 passed; 0 failed" ;; esac`;
      runner(broken, { ONLY: "vm-lab per-machine", CYBERCTF_TEST_VM_PROVIDER: "libvirt" }, ["--results", lab]);
      expect(
        readFileSync(lab, "utf8")
          .trim()
          .split("\n")
          .map((l) => (JSON.parse(l) as Record<string, unknown>).result),
      ).toEqual(["fail", "pass"]);
    },
  );

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
