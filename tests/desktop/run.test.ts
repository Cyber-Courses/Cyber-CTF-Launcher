import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

// The desktop end-to-end's cases (cases.tsv), held to its runners and its specs, and what the
// runners print for a caller: the listing, and the JSON records (run.ps1's, through lib/suite.ps1;
// run.sh's are scripts/lib/suite.sh's, tested there). No app is started.

const HERE = import.meta.dirname;
const read = (file: string) => readFileSync(join(HERE, file), "utf8");
const pwsh = spawnSync("pwsh", ["-NoProfile", "-Command", "exit 0"]).status === 0;
const dirs: string[] = [];

afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

interface Case {
  case: string;
  spec: string;
  form: string;
  knobs: string[];
}

/** cases.tsv's rows. */
function cases(): Case[] {
  return read("cases.tsv")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => {
      const [id = "", spec = "", form = "", knobs = ""] = l.split("\t");
      return { case: id, spec, form: form === "-" ? "" : form, knobs: knobs === "-" ? [] : knobs.split(" ") };
    });
}

/** The knobs a spec reads: from the environment, or through support.mjs's `localLab` and `CATALOGUE_LAB`. */
function knobsOf(spec: string): string[] {
  const code = read(`specs/${spec}.mjs`);
  const knobs = new Set([...code.matchAll(/\benv\.(CYBERCTF_TEST_[A-Z0-9_]+)/g)].map((m) => m[1]));
  if (code.includes("localLab(")) knobs.add("CYBERCTF_TEST_LOCAL_LAB");
  if (/\bCATALOGUE_LAB\b/.test(code)) knobs.add("CYBERCTF_TEST_CATALOGUE_LAB");
  return [...knobs].sort();
}

/** The case ids a runner's code runs or skips, by the patterns `patterns` (each one id per match). */
function idsIn(code: string, patterns: RegExp[], lists: RegExp[]): Set<string> {
  const ids = new Set<string>();
  for (const p of patterns) for (const m of code.matchAll(p)) ids.add(m[1]);
  for (const p of lists) for (const m of code.matchAll(p)) for (const w of m[1].match(/[a-z][a-z-]+/g) ?? []) ids.add(w);
  return ids;
}

describe("the desktop cases", () => {
  it("name each spec once by its file, and each lab-form case its form", () => {
    const specs = readdirSync(join(HERE, "specs")).map((f) => f.replace(/\.mjs$/, ""));
    const all = cases();
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
    for (const c of cases()) expect([...c.knobs].sort(), c.case).toEqual(knobsOf(c.spec));
  });

  it("are the cases run.sh and run.ps1 run, all of them", () => {
    const ids = new Set(cases().map((c) => c.case));
    const sh = idsIn(
      read("run.sh"),
      [/^\s*(?:spec|skip_case|run_logged) "?([a-z][a-z-]+)"?(?: |$)/gm, /^\s*run_spec ([a-z][a-z-]+) /gm, /run_spec "?([a-z][a-z-]+)"? [a-z-]+ "\$/gm],
      [/for s in ([a-z -]+); do/g, /DOCKER_SPECS="([a-z -]+)"/g],
    );
    sh.delete("final");
    const ps = idsIn(
      read("run.ps1"),
      [/(?:Invoke-Spec|Invoke-Logged|Skip-Spec) "([a-z][a-z-]+)"/g],
      [/foreach \(\$s in ("[^)]*")\)/g, /\$dockerSpecs = ("[^\n]*)/g],
    );
    expect(sh).toEqual(ids);
    expect(ps).toEqual(ids);
  });
});

describe("--list", () => {
  const listed = (out: string) =>
    out
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as Case & { suite: string });

  it("prints a JSON line per case, as cases.tsv has it", () => {
    const r = spawnSync("bash", [join(HERE, "run.sh"), "--list"], { encoding: "utf8" });
    expect(r.status).toBe(0);
    expect(listed(r.stdout)).toEqual(cases().map((c) => ({ suite: "desktop", ...c })));
  });

  it.skipIf(!pwsh)("prints the same from run.ps1", () => {
    const sh = spawnSync("bash", [join(HERE, "run.sh"), "--list"], { encoding: "utf8" });
    const ps = spawnSync("pwsh", ["-NoProfile", "-File", join(HERE, "run.ps1"), "-List"], { encoding: "utf8" });
    expect(ps.status).toBe(0);
    expect(listed(ps.stdout)).toEqual(listed(sh.stdout));
  });
});

describe.skipIf(!pwsh)("run.ps1's records", () => {
  it("are a line per case and per check, with its form, provider and log, the check lines not counted", () => {
    const dir = mkdtempSync(join(tmpdir(), "desktop-results-"));
    dirs.push(dir);
    const script = join(dir, "records.ps1");
    writeFileSync(
      script,
      [
        `. "${join(HERE, "lib", "suite.ps1")}"`,
        `Read-Case "${join(HERE, "cases.tsv")}"`,
        `Initialize-ResultsFile "${join(dir, "out", "results.jsonl")}"`,
        `$env:CYBERCTF_TEST_VM_PROVIDER = "hyperv"`,
        `Add-Result "lab-form-vm" "FAIL" 12 "vm.log (exit 1)" (Get-CaseForm "lab-form-vm") (Get-CaseProvider "lab-form-vm") "vm.log"`,
        `Add-CheckResult "lab-form-vm" @('ok: start (3s) it "runs" here', 'FAILED: stop (9s) left a VM', 'skipped: spec (0s) whole') "vm" "hyperv" "vm.log"`,
        `Add-Result "settings" "skip" 0 "docker is not running" (Get-CaseForm "settings") (Get-CaseProvider "settings")`,
        `"$($script:Failed) $($script:Skipped) $($script:SummaryLines.Count)"`,
      ].join("\n"),
    );
    const r = spawnSync("pwsh", ["-NoProfile", "-File", script], { encoding: "utf8" });
    expect(r.stderr).toBe("");
    expect(r.stdout.trim()).toBe("1 1 4");
    const records = readFileSync(join(dir, "out", "results.jsonl"), "utf8")
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as unknown);
    expect(records).toEqual([
      { suite: "desktop", case: "lab-form-vm", provider: "hyperv", form: "vm", result: "fail", reason: "vm.log (exit 1)", seconds: 12, log: "vm.log" },
      { suite: "desktop", case: "lab-form-vm/start", provider: "hyperv", form: "vm", result: "pass", reason: "", seconds: 3, log: "vm.log" },
      { suite: "desktop", case: "lab-form-vm/stop", provider: "hyperv", form: "vm", result: "fail", reason: "left a VM", seconds: 9, log: "vm.log" },
      { suite: "desktop", case: "settings", provider: "", form: "", result: "skip", reason: "docker is not running", seconds: 0, log: "" },
    ]);
    expect(existsSync(join(dir, "out"))).toBe(true);
  });
});

describe("the run's own folder", () => {
  /** A temporary folder the run's folder is made in, and a session file to place. */
  function scratch() {
    const dir = realpathSync(mkdtempSync(join(tmpdir(), "desktop-root-")));
    dirs.push(dir);
    const session = join(dir, "session.json");
    writeFileSync(session, '{"accessToken":"t"}');
    return { dir, session };
  }

  it.skipIf(process.platform === "win32")("is made fresh by run.sh, holds the session where the app reads it, and goes unless kept", () => {
    const { dir, session } = scratch();
    const lib = join(HERE, "lib", "run-root.sh");
    const sh = (script: string, env: Record<string, string> = {}) =>
      spawnSync("bash", ["-c", `. "${lib}"; ${script}`], { encoding: "utf8", env: { ...process.env, TMPDIR: dir, ...env } });
    const made = sh("run_root_make").stdout.trim();
    expect(made.startsWith(join(dir, "cyberctf-e2e."))).toBe(true);
    expect(readdirSync(made)).toEqual([]);
    expect(sh("run_root_make").stdout.trim()).not.toBe(made);
    expect(sh(`run_root_place_session "${made}" "${session}"`).status).toBe(0);
    const placed = join(made, "home", ".cyberctf", "dev-session.json");
    expect(readFileSync(placed, "utf8")).toBe('{"accessToken":"t"}');
    expect(statSync(placed).mode & 0o077).toBe(0);
    expect(sh(`run_root_drop "${made}"`, { CYBERCTF_TEST_DESKTOP_KEEP: "1" }).stdout).toContain(`kept: the run's folder ${made}`);
    expect(existsSync(made)).toBe(true);
    expect(sh(`run_root_drop "${made}"`).stdout).toBe("");
    expect(existsSync(made)).toBe(false);
  });

  it.skipIf(!pwsh)("is made fresh by run.ps1 the same way", () => {
    const { dir, session } = scratch();
    const script = join(dir, "root.ps1");
    writeFileSync(
      script,
      [
        `. "${join(HERE, "lib", "suite.ps1")}"`,
        `$a = Initialize-RunRoot`,
        `$b = Initialize-RunRoot`,
        `Copy-RunSession $a "${session}"`,
        `$placed = Get-Content -Raw (Join-Path $a "home/.cyberctf/dev-session.json")`,
        `$env:CYBERCTF_TEST_DESKTOP_KEEP = "1"`,
        `$kept = Clear-RunRoot $a`,
        `$keptThere = Test-Path $a`,
        `$env:CYBERCTF_TEST_DESKTOP_KEEP = ""`,
        `$gone = Clear-RunRoot $a`,
        `Clear-RunRoot $b | Out-Null`,
        `ConvertTo-Json -Compress @{ fresh = ($a -ne $b); name = (Split-Path -Leaf $a); placed = $placed; kept = $kept; keptThere = $keptThere; gone = ($null -eq $gone); left = ((Test-Path $a) -or (Test-Path $b)) }`,
      ].join("\n"),
    );
    const r = spawnSync("pwsh", ["-NoProfile", "-File", script], { encoding: "utf8" });
    expect(r.stderr).toBe("");
    const out = JSON.parse(r.stdout) as Record<string, unknown>;
    expect(out).toMatchObject({ fresh: true, placed: '{"accessToken":"t"}', keptThere: true, gone: true, left: false });
    expect(String(out.name)).toMatch(/^cyberctf-e2e-[a-z0-9]+$/);
    expect(String(out.kept)).toContain("kept: the run's folder");
  });

  it("is given to the app and the specs, and every clean-up goes through what the run recorded", () => {
    for (const runner of [read("run.sh"), read("run.ps1")]) {
      expect(runner).toMatch(/CYBERCTF_WDIO_ROOT/);
      expect(runner).toMatch(/owned\.mjs/);
      expect(runner).toMatch(/ record "?\$(RUN_ROOT|RunRoot)/);
      // No clean-up by a name pattern, nor of every copy of the app's binary.
      expect(runner).not.toMatch(/--filter "name=|pkill -f "\$APP_BIN"|\$_\.Path -eq \$App \} \| Stop-Process/);
      // Nothing of the user's: their data folder, their session file.
      expect(runner).not.toMatch(/Application Support|XDG_DATA_HOME|USERPROFILE|\$HOME\/\.cyberctf/);
    }
  });
});
