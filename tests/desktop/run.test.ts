import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  DEAD_APP,
  checkLines,
  declarationsOf,
  failureTail,
  groupsOf,
  jsonRecord,
  needsState,
  planProblems,
  providersOf,
  readPlan,
  selectCases,
  skipReason,
  withoutSecrets,
} from "./lib/plan.mjs";

// The desktop end-to-end's plan (cases.tsv, the specs' declared needs, the groups; lib/plan.mjs)
// held to the specs, and its runner (run.mjs) run for real on a fake plan: fake specs, a fake
// app answering (or not) on the WebDriver port, a fake docker. No real app is started.

const HERE = import.meta.dirname;
const RUNNER = join(HERE, "run.mjs");
const unix = process.platform !== "win32";
const dirs: string[] = [];

afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

interface Case {
  case: string;
  spec: string;
  form: string;
  knobs: string[];
  groups: string[];
  needs: string[];
  provides: string[];
}

/** The knobs a spec reads: from the environment, or through support.mjs's `localLab` and `CATALOGUE_LAB`. */
function knobsOf(spec: string): string[] {
  const code = readFileSync(join(HERE, "specs", `${spec}.mjs`), "utf8");
  const knobs = new Set([...code.matchAll(/\benv\.(CYBERCTF_TEST_[A-Z0-9_]+)/g)].map((m) => m[1]));
  if (code.includes("localLab(")) knobs.add("CYBERCTF_TEST_LOCAL_LAB");
  if (/\bCATALOGUE_LAB\b/.test(code)) knobs.add("CYBERCTF_TEST_CATALOGUE_LAB");
  return [...knobs].sort();
}

const plan = () => readPlan(HERE);

describe("the desktop cases", () => {
  it("name each spec once by its file, and each lab-form case its form", () => {
    const specs = readdirSync(join(HERE, "specs")).map((f) => f.replace(/\.mjs$/, ""));
    const all = plan().cases;
    expect(new Set(all.map((c) => c.spec))).toEqual(new Set(specs));
    expect(new Set(all.map((c) => c.case)).size).toBe(all.length);
    for (const c of all) expect(c.case === c.spec || c.case === `${c.spec}-${c.form}`, c.case).toBe(true);
  });

  it("are specs Node can load, each of them", () => {
    // Nothing else reads them before the runner starts one (ESLint takes the TypeScript tests
    // only): cleanup.mjs declared `listed` twice and never ran.
    for (const spec of readdirSync(join(HERE, "specs"))) {
      const check = spawnSync(process.execPath, ["--check", join(HERE, "specs", spec)], { encoding: "utf8" });
      expect(check.status, `${spec}: ${check.stderr}`).toBe(0);
    }
  });

  it("list the knobs each spec reads", () => {
    for (const c of plan().cases) expect([...c.knobs].sort(), c.case).toEqual(knobsOf(c.spec));
  });

  it("each declare their needs, in a plan that holds together", () => {
    const p = plan();
    expect(planProblems(p)).toEqual([]);
    for (const spec of readdirSync(join(HERE, "specs"))) expect(readFileSync(join(HERE, "specs", spec), "utf8"), spec).toMatch(/^export const needs = /m);
    // The fixture lab's start is proven once: by launch, which the specs starting it again need.
    expect(p.providers.get("running-lab:fixture")).toBe("launch");
    for (const name of ["restart", "stop", "attack-box", "stop-during-start", "park-resume"])
      expect(p.cases.find((c) => c.case === name)?.needs, name).toContain("running-lab:fixture");
    // One VM lab start on the hypervisor proves the others can try.
    expect(p.providers.get("vm-start")).toBe("lab-form-docker-vm");
    for (const name of ["lab-form-vm", "lab-form-per-machine", "screens"]) expect(p.cases.find((c) => c.case === name)?.needs, name).toContain("vm-start");
    expect(p.cases.find((c) => c.case === "lab-form-docker-vm")?.needs).toEqual(["session", "hypervisor"]);
  });

  it("fall into four groups, each holding the providers its cases need, cleanup last", () => {
    const p = plan();
    expect(groupsOf(p.cases)).toEqual(["shell", "lab", "resilience", "vm"]);
    expect(p.cases.at(-1)?.case).toBe("cleanup");
    expect(selectCases(p.cases, { group: "resilience" }).map((c) => c.case)).toEqual([
      "launch",
      "kill-mid-deploy",
      "stop-during-start",
      "double-start",
      "close-mid-deploy",
      "kill-worker",
      "park-resume",
      "reimport-running",
    ]);
    expect(selectCases(p.cases, { only: ["smoke", "stop"] }).map((c) => c.case)).toEqual(["smoke", "stop"]);
  });

  it("are the cases run.sh and run.ps1 list through run.mjs, with their groups and needs", () => {
    const sh = spawnSync("bash", [join(HERE, "run.sh"), "--list"], { encoding: "utf8" });
    expect(sh.status, sh.stderr).toBe(0);
    const listed = sh.stdout
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as Case & { suite: string });
    expect(listed).toEqual(plan().cases.map((c) => ({ suite: "desktop", ...c })));
    const group = spawnSync("bash", [join(HERE, "run.sh"), "--group", "shell", "--list"], { encoding: "utf8" });
    expect(group.stdout.trim().split("\n").length).toBe(12);
    for (const runner of [readFileSync(join(HERE, "run.sh"), "utf8"), readFileSync(join(HERE, "run.ps1"), "utf8")]) {
      expect(runner).toMatch(/run\.mjs/);
      // Nothing of the user's: their data folder, their session file, a clean-up by name.
      expect(runner).not.toMatch(/Application Support|XDG_DATA_HOME|USERPROFILE|\$HOME\/\.cyberctf|--filter "name=/);
    }
  });
});

describe("the plan's pieces", () => {
  it("read a spec's declarations, by form when it runs once per form", () => {
    expect(declarationsOf('export const needs = ["engine"];\nexport const provides = ["x"];')).toEqual({ needs: ["engine"], provides: ["x"] });
    expect(declarationsOf("const r = 1;")).toEqual({ needs: [], provides: [] });
    const byForm = 'export const needs = { container: ["session"], vm: ["vm-start"] };\nexport const provides = { container: ["catalogue-lab"] };';
    expect(declarationsOf(byForm, "container")).toEqual({ needs: ["session"], provides: ["catalogue-lab"] });
    expect(declarationsOf(byForm, "vm")).toEqual({ needs: ["vm-start"], provides: [] });
  });

  it("name what is wrong with a plan", () => {
    const c = (name: string, needs: string[], provides: string[] = [], groups = ["g"]) => ({
      case: name,
      spec: name,
      form: "",
      knobs: [],
      needs,
      provides,
      groups,
    });
    const cases = [c("b", ["x"]), c("a", [], ["x"]), c("d", ["nothing"]), c("e", ["y"], ["y"]), c("f", ["x"], [], ["h"]), c("i", [], [], [])];
    expect(planProblems({ cases, providers: providersOf(cases) })).toEqual([
      "b needs x, but its provider a comes after it",
      "d needs nothing, which nothing provides",
      "e needs y, which it provides itself",
      "group h has f, which needs x, but not its provider a",
      "i is in no group",
    ]);
  });

  it("follow a need through its provider's verdict, and take an absent provider as met", () => {
    const state = needsState(new Map([["running-lab:fixture", "launch"]]));
    expect(state.unmet("running-lab:fixture")).toBeNull();
    state.verdict("launch", "FAIL", "the seeded lab never showed as running on Overview");
    expect(state.unmet("running-lab:fixture")).toBe(
      "needs running-lab:fixture, which launch didn't provide (failed: the seeded lab never showed as running on Overview)",
    );
    state.verdict("launch", "skip", "needs engine: no Docker engine answers here");
    expect(state.unmet("running-lab:fixture")).toBe(
      "needs running-lab:fixture, which launch didn't provide (skipped: needs engine: no Docker engine answers here)",
    );
    state.verdict("launch", "ok", "");
    expect(state.unmet("running-lab:fixture")).toBeNull();
    expect(state.checked("engine")).toBe(false);
    state.provided("engine", "no Docker engine answers here");
    expect(state.checked("engine")).toBe(true);
    expect(state.unmet("engine")).toBe("needs engine: no Docker engine answers here");
    expect(state.blocked({ needs: ["session", "engine"] })).toBe("needs engine: no Docker engine answers here");
    state.provided("engine", null);
    expect(state.blocked({ needs: ["engine"] })).toBeNull();
  });

  it("read a spec's check lines, a whole skip's reason, a dead app, a failure's tail", () => {
    const log = "noise\nok: start (3s) it runs\nFAILED: stop (9s) left a VM\r\nskipped: spec (0s) no lab folder\n";
    expect(checkLines(log)).toEqual([
      { status: "ok", id: "start", seconds: 3, text: "it runs" },
      { status: "FAIL", id: "stop", seconds: 9, text: "left a VM" },
      { status: "skip", id: "spec", seconds: 0, text: "no lab folder" },
    ]);
    expect(skipReason(log)).toBe("no lab folder");
    expect(skipReason("ok: a (1s) b")).toBeNull();
    expect(DEAD_APP.test("Error: connect ECONNREFUSED 127.0.0.1:4445")).toBe(true);
    expect(DEAD_APP.test("app exited (code 1) before the server came up:")).toBe(true);
    expect(DEAD_APP.test("FAILED: launch (3s) lab never ran")).toBe(false);
    expect(failureTail("a\nb\n\napp out tail: thread 'main' panicked\n  at x\n")).toBe("thread 'main' panicked | at x");
    expect(failureTail("one\ntwo\nthree", 2)).toBe("two | three");
  });

  it("write the record every runner writes", () => {
    expect(
      JSON.parse(jsonRecord({ name: "lab-form-vm", status: "FAIL", seconds: 12, reason: "vm.log (exit 1)", form: "vm", provider: "hyperv", log: "vm.log" })),
    ).toEqual({
      suite: "desktop",
      case: "lab-form-vm",
      provider: "hyperv",
      form: "vm",
      result: "fail",
      reason: "vm.log (exit 1)",
      seconds: 12,
      log: "vm.log",
    });
    expect(JSON.parse(jsonRecord({ name: "smoke", status: "ok", seconds: 3, reason: "the log" }))).toMatchObject({
      result: "pass",
      reason: "",
      provider: "",
      form: "",
    });
  });
});

/**
 * A fake plan: `specs` as `{ name: { needs, provides, groups, body } }`, each spec a script that
 * prints its check lines and exits as `body` says (`FAKE_SPEC_*` in the environment steers it).
 */
function fakePlan(specs: Record<string, { needs?: string[]; provides?: string[]; groups?: string[]; body: string }>): string {
  const dir = mkdtempSync(join(tmpdir(), "desktop-plan-"));
  dirs.push(dir);
  mkdirSync(join(dir, "specs"));
  const rows = Object.entries(specs).map(([name, s]) => {
    writeFileSync(
      join(dir, "specs", `${name}.mjs`),
      `export const needs = ${JSON.stringify(s.needs ?? [])};\nexport const provides = ${JSON.stringify(s.provides ?? [])};\n${s.body}\n`,
    );
    return `${name}\t${name}\t-\t-\t${(s.groups ?? ["all"]).join(" ")}`;
  });
  writeFileSync(join(dir, "cases.tsv"), `${rows.join("\n")}\n`);
  return dir;
}

/** A fake app: a script that serves /status on TAURI_WEBDRIVER_PORT, unless the `dead` file is there. */
function fakeApp(dir: string): string {
  const server = join(dir, "app.mjs");
  writeFileSync(
    server,
    `import { createServer } from "node:http";
import { existsSync } from "node:fs";
if (existsSync(${JSON.stringify(join(dir, "dead"))})) process.exit(3);
createServer((req, res) => { res.end("{}"); }).listen(Number(process.env.TAURI_WEBDRIVER_PORT), "127.0.0.1");
setInterval(() => {}, 1000);`,
  );
  const bin = join(dir, "cyberctf-desktop");
  writeFileSync(bin, `#!/bin/sh\nexec "${process.execPath}" "${server}"\n`);
  chmodSync(bin, 0o755);
  return bin;
}

/** The runner on a fake plan with a fake app and a fake docker (`dockerUp`: whether it answers). */
function runRunner(planDir: string, args: string[], { dockerUp = true, env = {} }: { dockerUp?: boolean; env?: Record<string, string> } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "desktop-run-"));
  dirs.push(dir);
  const app = fakeApp(dir);
  if (env.FAKE_APP_DEAD) writeFileSync(join(dir, "dead"), "");
  // A docker that answers `info` as asked and knows no Compose project; a vagrant with no boxes.
  writeFileSync(join(dir, "docker"), `#!/bin/sh\ncase "$1" in info) exit ${dockerUp ? "0" : "1"} ;; compose) echo '[]' ;; *) exit 0 ;; esac\n`);
  writeFileSync(join(dir, "vagrant"), "#!/bin/sh\nexit 0\n");
  chmodSync(join(dir, "docker"), 0o755);
  chmodSync(join(dir, "vagrant"), 0o755);
  const results = join(dir, "out", "results.jsonl");
  const lab = join(dir, "lab");
  mkdirSync(lab);
  writeFileSync(join(lab, "isoloom.yml"), "name: Fixture Lab\n");
  const clean = {
    ...process.env,
    PATH: `${dir}:${process.env.PATH ?? ""}`,
    ONLY: "",
    SKIP: "",
    STRICT: "",
    CYBERCTF_TEST_DESKTOP_APP: app,
    CYBERCTF_TEST_DESKTOP_LOGS: join(dir, "logs"),
    CYBERCTF_TEST_DESKTOP_TIMEOUT: "4",
    CYBERCTF_TEST_LOCAL_LAB: lab,
    CYBERCTF_TEST_ACCOUNT_SESSION: "",
    CYBERCTF_TEST_VM_PROVIDER: "",
    TAURI_WEBDRIVER_PORT: String(40000 + Math.floor(Math.random() * 20000)),
    TMPDIR: dir,
    ...env,
  };
  const r = spawnSync(process.execPath, [RUNNER, "--plan", planDir, "--results", results, ...args], { env: clean, encoding: "utf8", timeout: 60000 });
  const records = existsSync(results)
    ? readFileSync(results, "utf8")
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l) as Record<string, unknown>)
    : [];
  return { status: r.status, out: r.stdout + r.stderr, records, dir };
}

const verdicts = (records: Record<string, unknown>[]): Record<string, [unknown, unknown]> =>
  Object.fromEntries(records.filter((r) => !String(r.case).includes("/")).map((r): [string, [unknown, unknown]] => [String(r.case), [r.result, r.reason]]));

describe.skipIf(!unix)("run.mjs on a fake plan", () => {
  const ok = 'console.log("ok: it (1s) fine"); process.exit(0);';

  it("runs each case once, a need the runner can't provide skipping its cases with why, at once", () => {
    const planDir = fakePlan({
      a: { body: ok },
      b: { needs: ["engine"], body: ok },
      c: { needs: ["session"], body: ok },
      d: { needs: ["fixture-lab"], body: ok },
    });
    const r = runRunner(planDir, ["--all"], { dockerUp: false });
    expect(r.status, r.out).toBe(0);
    expect(verdicts(r.records)).toEqual({
      a: ["pass", ""],
      b: ["skip", expect.stringMatching(/^needs engine: no Docker engine answers here/) as unknown],
      c: ["skip", "needs session: no session file: give CYBERCTF_TEST_ACCOUNT_SESSION to the runner (a catalogue lab starts only signed in)"],
      d: ["pass", ""],
    });
    expect(r.records.find((x) => x.case === "a/it")).toMatchObject({ result: "pass", seconds: 1 });
    expect(r.records.find((x) => x.case === "b")).toMatchObject({ seconds: 0 });
    expect(r.out).toContain("==== b: skipped (needs engine");
    expect(r.out).toMatch(/\n1 failed, 2 skipped|\n0 failed, 2 skipped/);
    expect(existsSync(join(r.dir, "logs", "a.log"))).toBe(true);
    // The run's folder went with the run.
    expect(readdirSync(r.dir).filter((f) => f.startsWith("cyberctf-e2e."))).toEqual([]);
  });

  it("a need a case provides: its failure skips every case needing it, naming the failure; a pass meets it", () => {
    const planDir = fakePlan({
      launch: {
        needs: ["engine"],
        provides: ["running-lab:fixture"],
        body: 'console.log("FAILED: launch (2s) the seeded lab never showed as running on Overview"); console.log("app out tail: thread main panicked"); process.exit(1);',
      },
      restart: { needs: ["running-lab:fixture"], body: ok },
      stop: { needs: ["running-lab:fixture"], body: ok },
      other: { body: ok },
    });
    const failed = runRunner(planDir, ["--all"]);
    expect(failed.status).toBe(1);
    expect(verdicts(failed.records)).toEqual({
      launch: ["fail", "the seeded lab never showed as running on Overview (exit 1)"],
      restart: ["skip", "needs running-lab:fixture, which launch didn't provide (failed: the seeded lab never showed as running on Overview (exit 1))"],
      stop: ["skip", "needs running-lab:fixture, which launch didn't provide (failed: the seeded lab never showed as running on Overview (exit 1))"],
      other: ["pass", ""],
    });
    writeFileSync(join(planDir, "specs", "launch.mjs"), `export const needs = ["engine"];\nexport const provides = ["running-lab:fixture"];\n${ok}`);
    const passed = runRunner(planDir, ["--all"]);
    expect(passed.status, passed.out).toBe(0);
    expect(Object.values(verdicts(passed.records)).map((v) => v[0])).toEqual(["pass", "pass", "pass", "pass"]);
  });

  it("takes a group, --specs as ONLY, SKIP, and a whole spec's skip", () => {
    const planDir = fakePlan({
      a: { groups: ["one"], body: ok },
      b: { groups: ["one", "two"], body: 'console.log("skipped: spec (0s) no hypervisor named"); process.exit(2);' },
      c: { groups: ["two"], body: ok },
    });
    const two = runRunner(planDir, ["--group", "two"]);
    expect(Object.keys(verdicts(two.records))).toEqual(["b", "c"]);
    expect(verdicts(two.records).b).toEqual(["skip", "no hypervisor named"]);
    const only = runRunner(planDir, ["--specs", "a c"], { env: { SKIP: "c" } });
    expect(Object.keys(verdicts(only.records))).toEqual(["a"]);
    const strict = runRunner(planDir, ["--all"], { env: { STRICT: "1" } });
    expect(strict.status).toBe(1);
    expect(runRunner(planDir, ["--group", "nine"]).status).toBe(2);
  });

  it("kills a spec past its cap, the verdict saying so", () => {
    const planDir = fakePlan({ slow: { body: 'console.log("ok: first (1s) done"); setTimeout(() => {}, 60000);' }, next: { body: ok } });
    const r = runRunner(planDir, ["--all"]);
    expect(r.status).toBe(1);
    expect(verdicts(r.records).slow).toEqual(["fail", expect.stringMatching(/^exceeded its 4 s: /) as unknown]);
    expect(verdicts(r.records).next).toEqual(["pass", ""]);
  });

  it("starts a dead app again once, and ends the run with the reason when it dies again", () => {
    // The spec kills the app it was given (the fake's `dead` file), then reports WebDriver refused.
    const dies = `import { writeFileSync } from "node:fs"; writeFileSync(process.env.FAKE_DEAD_FILE, ""); console.log("FAILED: it (1s) Error: connect ECONNREFUSED 127.0.0.1:4445"); process.exit(1);`;
    const planDir = fakePlan({ a: { body: ok }, b: { body: dies }, c: { body: ok }, d: { body: dies }, e: { body: ok } });
    // Dead for good after b: the restart attempt fails, and c, d, e are skipped with why.
    const dir = mkdtempSync(join(tmpdir(), "desktop-dead-"));
    dirs.push(dir);
    const r = runRunner(planDir, ["--all"], { env: { FAKE_DEAD_FILE: join(dir, "never-read") } });
    expect(r.status).toBe(1);
    expect(verdicts(r.records).a).toEqual(["pass", ""]);
    expect(verdicts(r.records).b[0]).toBe("fail");
    expect(r.out).toContain("==== b: the app died (");
    expect(r.out).toContain("starting it again once");
  });

  it("ends the run at once when the app can't come up again after it died", () => {
    const planDir = fakePlan({
      a: { body: ok },
      b: {
        body: 'import { writeFileSync } from "node:fs"; writeFileSync(process.env.FAKE_DEAD_FILE, ""); console.log("FAILED: it (1s) connect ECONNREFUSED"); process.exit(1);',
      },
      c: { body: ok },
    });
    // The fake app reads `dead` beside itself: the spec writes it, so the restart attempt fails.
    const dir = mkdtempSync(join(tmpdir(), "desktop-run-"));
    dirs.push(dir);
    const app = fakeApp(dir);
    const r = runRunner(planDir, ["--all"], { env: { CYBERCTF_TEST_DESKTOP_APP: app, FAKE_DEAD_FILE: join(dir, "dead") } });
    expect(r.status).toBe(1);
    expect(verdicts(r.records).b[0]).toBe("fail");
    expect(verdicts(r.records).c).toEqual([
      "skip",
      expect.stringMatching(/^the app died \(.*\) and didn't come up again: the app exited \(code 3\)/) as unknown,
    ]);
  });

  it("refuses to run when the app has no WebDriver server", () => {
    const planDir = fakePlan({ a: { body: ok } });
    const r = runRunner(planDir, ["--all"], { env: { FAKE_APP_DEAD: "1" } });
    expect(r.status).toBe(2);
    expect(r.out).toContain("the app exited (code 3) before its WebDriver server answered");
    expect(r.records).toEqual([]);
  });

  it("places the session where the run's app reads it, and keeps the folder when asked", () => {
    const planDir = fakePlan({
      a: {
        body: 'import { readFileSync, statSync } from "node:fs"; const f = process.env.CYBERCTF_WDIO_ROOT + "/home/.cyberctf/dev-session.json"; console.log("ok: session (0s) " + readFileSync(f, "utf8") + " mode " + (statSync(f).mode & 0o077)); process.exit(0);',
      },
    });
    const dir = mkdtempSync(join(tmpdir(), "desktop-session-"));
    dirs.push(dir);
    writeFileSync(join(dir, "session.json"), '{"accessToken":"t"}');
    const r = runRunner(planDir, ["--all"], { env: { CYBERCTF_TEST_ACCOUNT_SESSION: join(dir, "session.json"), CYBERCTF_TEST_DESKTOP_KEEP: "1" } });
    expect(r.status, r.out).toBe(0);
    expect(r.records.find((x) => x.case === "a/session")?.reason).toBe("");
    expect(r.out).toContain('ok: session (0s) {"accessToken":"t"} mode 0');
    const kept = readdirSync(r.dir).filter((f) => f.startsWith("cyberctf-e2e."));
    expect(kept.length).toBe(1);
    expect(r.out).toContain(`kept: the run's folder`);
  });
});

describe("the apps' logs", () => {
  it("are kept without the lines that may name a secret", () => {
    expect(withoutSecrets("started\nthe launch token is abc\nPassword: x\nfailed: no engine\n")).toBe("started\nfailed: no engine\n");
    expect(withoutSecrets("")).toBe("");
  });

  it.skipIf(!unix)("are copied beside the specs' logs before the run's folder goes, the signed-out run's too, never home/", () => {
    const writes = `import { mkdirSync, writeFileSync } from "node:fs";
const root = process.env.CYBERCTF_WDIO_ROOT;
for (const dir of ["app/logs", "app/deploys", "signed-out/app/logs", "home/.cyberctf"]) mkdirSync(root + "/" + dir, { recursive: true });
writeFileSync(root + "/app/logs/cyberctf.log", "the app started\\nsession token t\\nthe lab failed\\n");
writeFileSync(root + "/app/deploys/local-lab.log", "deploying\\n");
writeFileSync(root + "/signed-out/app/logs/cyberctf.log", "signed out\\n");
writeFileSync(root + "/home/.cyberctf/secrets.log", "never copied\\n");
console.log("FAILED: it (1s) the lab failed"); process.exit(1);`;
    const r = runRunner(fakePlan({ a: { body: writes } }), ["--all"]);
    expect(r.status).toBe(1);
    const app = join(r.dir, "logs", "app");
    expect(readFileSync(join(app, "logs", "cyberctf.log"), "utf8")).toBe("the app started\nthe lab failed\n");
    expect(readFileSync(join(app, "deploys", "local-lab.log"), "utf8")).toBe("deploying\n");
    expect(readFileSync(join(app, "signed-out", "logs", "cyberctf.log"), "utf8")).toBe("signed out\n");
    expect(existsSync(join(r.dir, "logs", "home"))).toBe(false);
    expect(existsSync(join(app, "home"))).toBe(false);
    expect(r.out).toContain(`the apps' logs: 3 under ${app}`);
    // The run's folder went all the same.
    expect(readdirSync(r.dir).filter((f) => f.startsWith("cyberctf-e2e."))).toEqual([]);
  });
});
