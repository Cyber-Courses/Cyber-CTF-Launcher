import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
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
    CYBERCTF_TEST_ISOLOOM_DIR: dir,
    CYBERCTF_TEST_INTEGRATION_LOGS: dir,
    ...env,
  };
  const r = spawnSync("bash", [RUNNER, ...args], { env: { ...clean, PATH: `${dir}:${process.env.PATH ?? ""}` }, encoding: "utf8" });
  return { status: r.status, out: r.stdout + r.stderr, dir };
}

/** The caps the cases run under: one a case meant to end on its own never reaches (30 s), and one
 *  a case meant to be capped (its stand-ins sleep a minute) reaches only after its stand-ins have
 *  started (6 s). 2 s raced the start on an x64 Mac, where a freshly written script took longer
 *  than that to run the first time: a passing case read FAIL, a capped one had made nothing yet. */
const ENDS = "30";
const CAPPED = "6";

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

  it("is FAIL, saying so, when the test ran past its cap, cargo and what it ran killed", { timeout: 30000 }, () => {
    // A cargo that would run for a minute, under a cap of CAPPED s (the runner looks every 2 s).
    const r = runner(`echo "FAILED: nothing yet"; sleep 60 & echo $! > "$CYBERCTF_TEST_INTEGRATION_LOGS/sleeper"; wait`, {
      ONLY: "docker-lab-hello-stack",
      CYBERCTF_TEST_INTEGRATION_TIMEOUT: CAPPED,
    });
    expect(verdict(r.out, "docker-lab-hello-stack")).toBe("FAIL");
    expect(r.out).toMatch(new RegExp(`docker-lab-hello-stack +FAIL +\\d+s {2}exceeded its ${CAPPED}s \\(FAILED: nothing yet\\)`));
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
      `  hang) (trap '' TERM; exec sh -c 'sleep 60; :' "$TMPDIR/cyberctf-vm-2/qemu") & echo $! >"$CYBERCTF_TEST_INTEGRATION_LOGS/vm"; sleep 60 ;;`,
      "esac",
    ].join("\n");
    const ours = () => readdirSync(machine).filter((n) => n.startsWith("cyberctf"));
    for (const [kase, result] of [
      ["pass", "ok"],
      ["fail", "FAIL"],
      ["hang", "FAIL"],
    ]) {
      const r = runner(cargo, {
        ONLY: "docker-lab-hello-stack",
        CASE: kase,
        TMPDIR: machine,
        CYBERCTF_TEST_INTEGRATION_TIMEOUT: kase === "hang" ? CAPPED : ENDS,
      });
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
    // VirtualBox stood in for by a VBoxManage that knows that one VM and records what it's asked,
    // where VirtualBox's installer puts it on a Mac (/usr/local/bin), on none of the runner's PATH
    // (a Mac's outside a login shell: a capped scenario's VMs stayed registered there).
    const machine = folder();
    const root = folder();
    const id = "3c32882b-cf96-4a01-a754-ca7386e37b49";
    const vbox = [
      "#!/bin/sh",
      'echo "$*" >>"$CYBERCTF_TEST_INTEGRATION_LOGS/vbox.calls"',
      `case "$1 $2" in "showvminfo ${id}") echo 'name="solo-web - attackbox"' ;; showvminfo*) exit 1 ;; esac`,
    ].join("\n");
    const cargo = [
      'mkdir -p "$TOOL_ROOT/usr/local/bin"',
      `printf '%s\\n' '${vbox.replaceAll("'", "'\\''")}' >"$TOOL_ROOT/usr/local/bin/VBoxManage"`,
      'chmod +x "$TOOL_ROOT/usr/local/bin/VBoxManage"',
      'mkdir -p "$TMPDIR/cyberctf-box-1/.vagrant/machines/attackbox/virtualbox" "$TMPDIR/cyberctf-box-1/.vagrant/machines/web/virtualbox"',
      `echo ${id} >"$TMPDIR/cyberctf-box-1/.vagrant/machines/attackbox/virtualbox/id"`,
      'echo 00000000-0000-0000-0000-000000000000 >"$TMPDIR/cyberctf-box-1/.vagrant/machines/web/virtualbox/id"',
      'case "$CASE" in hang) sleep 60 ;; pass) echo "test result: ok. 1 passed; 0 failed" ;; esac',
    ].join("\n");
    const capped = runner(cargo, { ONLY: "attack-box-vm", CASE: "hang", TMPDIR: machine, TOOL_ROOT: root, CYBERCTF_TEST_INTEGRATION_TIMEOUT: CAPPED });
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
    const ended = runner(cargo, { ONLY: "attack-box-vm", CASE: "pass", TMPDIR: machine, TOOL_ROOT: root, CYBERCTF_TEST_INTEGRATION_TIMEOUT: ENDS });
    expect(verdict(ended.out, "attack-box-vm")).toBe("ok");
    expect(existsSync(join(ended.dir, "vbox.calls"))).toBe(false);
  });

  it("past its cap, ends its lab's UTM VMs too, defined in UTM's own library: each stopped, then deleted", { timeout: 60000 }, () => {
    // vagrant_utm keeps each machine's UTM VM by its uuid in the lab's .vagrant, the controller's
    // too; a UTM stood in for by a utmctl that records what it's asked, inside UTM's app, where
    // utmctl ships (on no PATH). A capped attack-box-vm on UTM left all three defined (2026-11-14).
    const machine = folder();
    const root = folder();
    const app = "$TOOL_ROOT/Applications/UTM.app/Contents/MacOS";
    const ids = {
      web: "5e1b0c1a-0d4c-4a35-9b0e-1f4e2d6a7b01",
      attackbox: "5e1b0c1a-0d4c-4a35-9b0e-1f4e2d6a7b02",
      "isoloom-controller": "5e1b0c1a-0d4c-4a35-9b0e-1f4e2d6a7b03",
    };
    const utmctl = ["#!/bin/sh", 'echo "$*" >>"$CYBERCTF_TEST_INTEGRATION_LOGS/utm.calls"'].join("\n");
    const cargo = [
      `mkdir -p "${app}"`,
      `printf '%s\\n' '${utmctl.replaceAll("'", "'\\''")}' >"${app}/utmctl"`,
      `chmod +x "${app}/utmctl"`,
      ...Object.entries(ids).flatMap(([m, id]) => [
        `mkdir -p "$TMPDIR/cyberctf-box-1/.vagrant/machines/${m}/utm"`,
        `echo ${id} >"$TMPDIR/cyberctf-box-1/.vagrant/machines/${m}/utm/id"`,
      ]),
      'case "$CASE" in hang) sleep 60 ;; pass) echo "test result: ok. 1 passed; 0 failed" ;; esac',
    ].join("\n");
    const capped = runner(cargo, { ONLY: "attack-box-vm", CASE: "hang", TMPDIR: machine, TOOL_ROOT: root, CYBERCTF_TEST_INTEGRATION_TIMEOUT: CAPPED });
    expect(verdict(capped.out, "attack-box-vm")).toBe("FAIL");
    const calls = readFileSync(join(capped.dir, "utm.calls"), "utf8").split("\n").filter(Boolean);
    expect([...calls].sort()).toEqual(
      Object.values(ids)
        .flatMap((id) => [`stop --kill ${id}`, `delete ${id}`])
        .sort(),
    );
    for (const id of Object.values(ids)) {
      expect(calls.indexOf(`stop --kill ${id}`), `${id} stopped, then deleted`).toBeLessThan(calls.indexOf(`delete ${id}`));
    }
    const log = readFileSync(join(capped.dir, "attack-box-vm.log"), "utf8");
    for (const id of Object.values(ids)) expect(log).toContain(`suite: stopping the scenario's UTM VM ${id}: stopped and deleted`);
    expect(readdirSync(machine).filter((n) => n.startsWith("cyberctf"))).toEqual([]);
    const ended = runner(cargo, { ONLY: "attack-box-vm", CASE: "pass", TMPDIR: machine, TOOL_ROOT: root, CYBERCTF_TEST_INTEGRATION_TIMEOUT: ENDS });
    expect(verdict(ended.out, "attack-box-vm")).toBe("ok");
    expect(existsSync(join(ended.dir, "utm.calls"))).toBe(false);
  });

  it("past its cap, takes down the Compose projects its labs started, and only those", { timeout: 60000 }, () => {
    // Compose labels a project's containers with its working folder: the scenario's are inside its
    // temp folder. A Docker stood in for by one that lists the scenario's project and another's,
    // and records what it's asked. A capped attack-box-docker left its project behind (2026-11-14).
    const machine = folder();
    const cargo = [
      'cat >"$(dirname "$0")/docker" <<STUB',
      "#!/bin/sh",
      'echo "\\$*" >>"\\$CYBERCTF_TEST_INTEGRATION_LOGS/docker.calls"',
      'case "\\$1" in',
      `  ps) printf 'cyberctf-box-13814\\t%s\\n' "$TMPDIR/cyberctf-box-1/runs/box/compose"; printf 'cyberctf-box-13814\\t%s\\n' "$TMPDIR/cyberctf-box-1/runs/box/compose"; printf 'players-own\\t/home/someone/lab\\n' ;;`,
      '  volume) [ "\\$2" = ls ] && echo cyberctf-box-13814_isoloom-probe ;;',
      "esac",
      "STUB",
      'chmod +x "$(dirname "$0")/docker"',
      'case "$CASE" in hang) sleep 60 ;; pass) echo "test result: ok. 1 passed; 0 failed" ;; esac',
    ].join("\n");
    const capped = runner(cargo, { ONLY: "attack-box-docker", CASE: "hang", TMPDIR: machine, CYBERCTF_TEST_INTEGRATION_TIMEOUT: CAPPED });
    expect(verdict(capped.out, "attack-box-docker")).toBe("FAIL");
    const calls = readFileSync(join(capped.dir, "docker.calls"), "utf8").split("\n").filter(Boolean);
    expect(calls.filter((c) => !c.startsWith("ps "))).toEqual([
      "compose -p cyberctf-box-13814 down -v --remove-orphans",
      "volume ls -q --filter label=com.docker.compose.project=cyberctf-box-13814",
      "volume rm -f cyberctf-box-13814_isoloom-probe",
    ]);
    expect(calls.join("\n"), "the player's own project is left alone").not.toContain("players-own");
    expect(readFileSync(join(capped.dir, "attack-box-docker.log"), "utf8")).toContain(
      "suite: taking down the scenario's Compose project cyberctf-box-13814: containers, networks and volumes",
    );
    const ended = runner(cargo, { ONLY: "attack-box-docker", CASE: "pass", TMPDIR: machine, CYBERCTF_TEST_INTEGRATION_TIMEOUT: ENDS });
    expect(verdict(ended.out, "attack-box-docker")).toBe("ok");
    expect(existsSync(join(ended.dir, "docker.calls"))).toBe(false);
  });

  it("gives its app's folders a root in its temp folder, so a capped one's lab runs and VirtualBox VMs go with it (#308)", { timeout: 60000 }, () => {
    // The test's app (TestApp::scenario) keeps a lab's runs in its data folder under the root, as
    // Isoloom lays them out: runs/<lab>/vagrant, Vagrant's state, naming the VirtualBox VM by its id.
    const machine = folder();
    const id = "3dba93af-6c1d-4f4e-9a59-0d1f0e6c2b17";
    const vbox = [
      "#!/bin/sh",
      'echo "$*" >>"$CYBERCTF_TEST_INTEGRATION_LOGS/vbox.calls"',
      `case "$1 $2" in "showvminfo ${id}") echo 'name="supplier-portal-api - web"' ;; showvminfo*) exit 1 ;; esac`,
    ].join("\n");
    const cargo = [
      'echo "$TMPDIR" >"$CYBERCTF_TEST_INTEGRATION_LOGS/temp"',
      'echo "$CYBERCTF_TEST_APP_ROOT" >"$CYBERCTF_TEST_INTEGRATION_LOGS/root"',
      `printf '%s\\n' '${vbox.replaceAll("'", "'\\''")}' >"$(dirname "$0")/VBoxManage"`,
      'chmod +x "$(dirname "$0")/VBoxManage"',
      'state="$CYBERCTF_TEST_APP_ROOT/com.cyberctf.unit-test.1-2/runs/pm-1/vagrant/machines/web/virtualbox"',
      `mkdir -p "$state" && echo ${id} >"$state/id"`,
      "sleep 60",
    ].join("\n");
    const r = runner(cargo, { ONLY: "per-machine", TMPDIR: machine, CYBERCTF_TEST_INTEGRATION_TIMEOUT: CAPPED });
    expect(verdict(r.out, "per-machine")).toBe("FAIL");
    const temp = readFileSync(join(r.dir, "temp"), "utf8").trim();
    expect(readFileSync(join(r.dir, "root"), "utf8").trim(), "the scenario's temp folder").toBe(temp);
    expect(readFileSync(join(r.dir, "vbox.calls"), "utf8").split("\n").filter(Boolean)).toEqual([
      `showvminfo ${id} --machinereadable`,
      `controlvm ${id} poweroff`,
      `unregistervm ${id} --delete`,
    ]);
    expect(existsSync(temp), `${temp} is left`).toBe(false);
  });

  it("runs the VM attack box bare, its toolset left out, and the container one as it is", () => {
    const seen = (only: string) => {
      const r = runner('echo "skip=${CYBERCTF_TEST_BOX_SKIP_TOOLS:-}" >"$CYBERCTF_TEST_INTEGRATION_LOGS/skip"; echo "test result: ok. 1 passed; 0 failed"', {
        ONLY: only,
      });
      expect(verdict(r.out, only), only).toBe("ok");
      return readFileSync(join(r.dir, "skip"), "utf8").trim();
    };
    expect(seen("attack-box-vm")).toBe("skip=1");
    expect(seen("attack-box-docker")).toBe("skip=");
  });

  it("makes its temp folder in TMPDIR, a folder on disk where /tmp is a tmpfs, but in /tmp for macOS's own", () => {
    const cargo = 'echo "$TMPDIR" >"$CYBERCTF_TEST_INTEGRATION_LOGS/temp"; echo "test result: ok. 1 passed; 0 failed"';
    const temp = (tmp: string) => {
      const r = runner(cargo, { ONLY: "docker-lab-hello-stack", TMPDIR: tmp });
      expect(verdict(r.out, "docker-lab-hello-stack"), tmp).toBe("ok");
      return readFileSync(join(r.dir, "temp"), "utf8").trim();
    };
    // The test rig's Linux clones: TMPDIR on their disk (Debian 13's /tmp is a tmpfs of half the RAM).
    const disk = mkdtempSync("/tmp/integration-test-");
    dirs.push(disk);
    expect(temp(disk)).toMatch(new RegExp(`^${realpathSync(disk)}/cyberctf-it\\.[A-Za-z0-9]{6}$`));
    // macOS's own (/var/folders/…/T/), too long a path for a socket made in the folder; one that
    // isn't a Unix path.
    for (const tmp of ["/var/folders/xy/abc/T/", "C:\\Users\\me\\AppData\\Local\\Temp"]) {
      expect(temp(tmp), tmp).toMatch(new RegExp(`^${realpathSync("/tmp")}/cyberctf-it\\.[A-Za-z0-9]{6}$`));
    }
  });

  it("takes --only <scenario>@<provider>: that scenario alone, on that hypervisor", () => {
    const out = join(folder(), "results.jsonl");
    runner(`echo "test result: ok. 1 passed; 0 failed"`, {}, ["--only", "vm-lab@virtualbox", "--results", out]);
    runner(`echo "test result: ok. 1 passed; 0 failed"`, {}, ["--only", "attack-box-docker@docker", "--only", "per-machine@hyperv", "--results", out]);
    const records = readFileSync(out, "utf8")
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as Record<string, unknown>);
    expect(records.map((r) => [r.case, r.provider, r.result])).toEqual([
      ["vm-lab", "virtualbox", "pass"],
      ["attack-box-docker", "", "pass"],
      ["per-machine", "hyperv", "pass"],
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
      // skipped with that line, the docker ones (no provider) and those before it run.
      const refusal = "FAILED: it starts: This lab can't run on UTM yet: `web` is on a network, and vagrant_utm gives a VM no private network.";
      const cargo = `case "$*" in *integration_tests::vm_lab::*) echo "${refusal.replaceAll("`", "\\`")}"; echo "test result: FAILED. 0 passed; 1 failed"; exit 101 ;; *) echo "test result: ok. 1 passed; 0 failed" ;; esac`;
      runner(
        cargo,
        {
          ONLY: "attack-box-vm docker-lab-hello-stack vm-lab per-machine replace-vm resilience-docker-port-taken resilience-vm-destroyed resilience-vm-killed-provisioning",
          CYBERCTF_TEST_VM_PROVIDER: "utm",
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
      for (const other of ["attack-box-vm", "docker-lab-hello-stack", "resilience-docker-port-taken"])
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
