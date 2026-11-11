#!/usr/bin/env node
// The desktop end-to-end's runner, the same on Linux, macOS and Windows: the real app (built with
// `--features wdio`) driven through its embedded WebDriver server, one spec a process, each with
// the needs it declares (`export const needs`, tests/desktop/lib/plan.mjs) met first, each capped,
// one verdict a spec and one line a check, then the summary. run.sh and run.ps1 build or unpack
// the app, serve a debug build's frontend, and start this; `pnpm test:desktop` goes through them.
//
//   node tests/desktop/run.mjs --all                        every case (the default)
//   node tests/desktop/run.mjs --group lab                  one group's cases (the rig's shards)
//   node tests/desktop/run.mjs --specs "smoke settings"     only these cases (as ONLY)
//   node tests/desktop/run.mjs --results results.jsonl      and a JSON record per case and check
//   node tests/desktop/run.mjs --list [--group <name>]      a JSON line per case: its spec, form, knobs, groups, needs
//
// Fail fast: a need the runner can't provide (the engine, the fixture lab, a session, a
// hypervisor, a server) skips every case needing it at once, with why; a need a case provides
// (`launch` proves the fixture lab starts here) is met by that case passing, so one failed start
// skips the cases that would start it again, each naming that failure. Every case is killed past
// its cap (5 minutes; the Machine page's 8). An app that dies (WebDriver refused) is started again
// once to see whether it can come up at all; a second death, or none, ends the run with that
// reason, the cases left skipped.
//
// Safe on a machine someone uses: the app keeps all of its own in a folder made for the run
// (CYBERCTF_WDIO_ROOT), removed at the end; every clean-up acts only on what the run made, by
// the records it wrote (tests/desktop/lib/owned.mjs).
//
// Knobs (environment, or the --knobs file; tests/README.md has each spec's):
//   CYBERCTF_TEST_DESKTOP_APP      the app, built with --features wdio (run.sh's --app, --package)
//   CYBERCTF_TEST_LOCAL_LAB        the lab folder to import (default: a checkout of the Invoice Portal API lab)
//   CYBERCTF_TEST_ACCOUNT_SESSION  a signed-in session (a dev-session.json), placed in the run's folder
//   CYBERCTF_TEST_VM_PROVIDER      the hypervisor for the VM specs (libvirt, virtualbox, hyperv, qemu, utm)
//   CYBERCTF_TEST_DESKTOP_TIMEOUT  seconds a spec may take (300; machine 480): the specs scale their waits to it
//   CYBERCTF_TEST_DESKTOP_LOGS     where the logs go (tests/desktop/.state/logs)
//   CYBERCTF_TEST_DESKTOP_KEEP     leave the fixture lab running and the run's folder in place at the end
//   TAURI_WEBDRIVER_PORT           the app's embedded WebDriver port (4445)
//   ONLY, SKIP, STRICT             as every suite runner (scripts/lib/suite.sh)
// Exit status: 0 nothing failed (and no skip under STRICT), 1 a spec failed, 2 the runner itself
// couldn't run. `--plan <dir>` reads another cases.tsv and specs/ (the runner's own tests).
import { execFileSync, spawn } from "node:child_process";
import {
  appendFileSync,
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { projects, record as recordMachine, runFolders, runTool } from "./lib/owned.mjs";
import {
  DEAD_APP,
  RUNNER_NEEDS,
  checkLines,
  failureTail,
  jsonRecord,
  needsState,
  planProblems,
  readPlan,
  selectCases,
  skipReason,
  summaryLine,
  withoutSecrets,
} from "./lib/plan.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..");
const WINDOWS = process.platform === "win32";
/** The seconds a spec may take; the Machine page's self-tests get more. */
const CAPS = { default: 300, machine: 480 };
/** How long the embedded WebDriver server may take to answer, at the start and after a death. */
const APP_UP_S = 60;
const UTMCTL = "/Applications/UTM.app/Contents/MacOS/utmctl";

function die(why) {
  console.error(`run.mjs: ${why}`);
  process.exit(2);
}
const say = (what) => console.log(`\n==== ${what}`);
const sleepSync = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

// --- Arguments -----------------------------------------------------------------------------
const args = { group: "", only: "", results: "", list: false, plan: HERE };
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  const next = () => process.argv[++i] ?? die(`${a} needs a value`);
  if (a === "--all") args.only = "";
  else if (a === "--group") args.group = next();
  else if (a === "--specs") args.only = next();
  else if (a === "--results") args.results = next();
  else if (a === "--list") args.list = true;
  else if (a === "--plan") args.plan = next();
  else if (a === "--knobs") {
    const file = next();
    if (!existsSync(file)) die(`no knobs file: ${file}`);
    for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
      if (!line || line.startsWith("#") || !line.includes("=")) continue;
      process.env[line.slice(0, line.indexOf("="))] = line.slice(line.indexOf("=") + 1);
    }
  } else die(`unknown argument: ${a} (see the header of tests/desktop/run.mjs)`);
}
if (args.only) process.env.ONLY = args.only;

const plan = readPlan(args.plan);
const problems = planProblems(plan);
if (problems.length) die(`the plan is inconsistent:\n  ${problems.join("\n  ")}`);
const cases = selectCases(plan.cases, { group: args.group });
if (args.group && cases.length === 0) die(`no group ${args.group} (--list says each case's)`);

if (args.list) {
  for (const c of cases)
    console.log(
      JSON.stringify({ suite: "desktop", case: c.case, spec: c.spec, form: c.form, knobs: c.knobs, groups: c.groups, needs: c.needs, provides: c.provides }),
    );
  process.exit(0);
}
// Loaded past the listing: it brings webdriverio, which a listing (the feature check's, the test
// rig's contract test's) needs no node_modules for.
const { hypervisorVms, localLabName } = await import("./lib/support.mjs");

// --- Settings ------------------------------------------------------------------------------
const listed = (word, list) => ` ${list} `.includes(` ${word} `);
const wanted = (name) => !(process.env.ONLY && !listed(name, process.env.ONLY)) && !(process.env.SKIP && listed(name, process.env.SKIP));
const APP = process.env.CYBERCTF_TEST_DESKTOP_APP || join(ROOT, "src-tauri", "target", "debug", `cyberctf-desktop${WINDOWS ? ".exe" : ""}`);
if (!existsSync(APP)) die(`app binary not found: ${APP} (build it with --features wdio, or give --app)`);
process.env.CYBERCTF_TEST_DESKTOP_APP = APP;
const PORT = Number(process.env.TAURI_WEBDRIVER_PORT || 4445);
process.env.TAURI_WEBDRIVER_PORT = String(PORT);
const LOGS = process.env.CYBERCTF_TEST_DESKTOP_LOGS || join(HERE, ".state", "logs");
mkdirSync(LOGS, { recursive: true });
const PROVIDER = process.env.CYBERCTF_TEST_VM_PROVIDER || "";
const KEEP = !!process.env.CYBERCTF_TEST_DESKTOP_KEEP;
const capOf = (c) => Number(process.env.CYBERCTF_TEST_DESKTOP_TIMEOUT || CAPS[c.case] || CAPS.default);

// --- Results -------------------------------------------------------------------------------
const results = { lines: [], failed: 0, skipped: 0, file: "" };
if (args.results) {
  results.file = resolve(args.results);
  mkdirSync(dirname(results.file), { recursive: true });
  appendFileSync(results.file, "");
}
/** One line of the summary, and its record; a check's (`counted` false) isn't counted again. */
function record({ name, status, seconds, detail, form = "", provider = "", log = "", counted = true }) {
  results.lines.push(summaryLine({ name, status, seconds, detail }));
  if (results.file) appendFileSync(results.file, `${jsonRecord({ name, status, seconds, reason: detail, form, provider, log })}\n`);
  if (!counted) return;
  if (status === "FAIL") results.failed += 1;
  if (status === "skip") results.skipped += 1;
}
const providerOf = (c) => (c.knobs.includes("CYBERCTF_TEST_VM_PROVIDER") ? PROVIDER : "");
function skipCase(c, why) {
  say(`${c.case}: skipped (${why})`);
  record({ name: c.case, status: "skip", seconds: 0, detail: why, form: c.form, provider: providerOf(c) });
}

// --- The run's own folder --------------------------------------------------------------------
// The app (a wdio build given it as CYBERCTF_WDIO_ROOT) keeps all of its own there: app/ its data,
// settings, caches, logs and webview data, home/ its session and secrets (src-tauri's
// platform/test_root.rs); records/ is the runner's (lib/owned.mjs). Made fresh under the system's
// temporary folder, its real path (Docker records the Compose files' paths; macOS's TMPDIR is
// under a link), removed at the end unless CYBERCTF_TEST_DESKTOP_KEEP.
const RUN_ROOT = realpathSync(mkdtempSync(join(tmpdir(), "cyberctf-e2e.")));
process.env.CYBERCTF_WDIO_ROOT = RUN_ROOT;
const FOLDERS = runFolders(RUN_ROOT);
/** The apps' logs (the app's own and its labs' deploy logs: every `*.log` under `<root>/app`, and
 *  under the signed-out run's), copied beside the specs' logs (`<logs>/app/`, their paths kept)
 *  before the run's folder goes, so a failed spec's evidence outlives it. Never `home/` (the
 *  session, the secrets); a line naming a password, a token or a key is left out (plan.mjs). */
function saveAppLogs() {
  const copied = [];
  for (const [from, to] of [
    [FOLDERS.app, join(LOGS, "app")],
    [join(RUN_ROOT, "signed-out", "app"), join(LOGS, "app", "signed-out")],
  ]) {
    for (const f of logFilesUnder(from)) {
      const dest = join(to, f.slice(from.length + 1));
      mkdirSync(dirname(dest), { recursive: true });
      writeFileSync(dest, withoutSecrets(readFileSync(f, "utf8")));
      copied.push(dest);
    }
  }
  if (copied.length) console.log(`the apps' logs: ${copied.length} under ${join(LOGS, "app")}`);
}
/** Every `*.log` under `dir`, up to 20 MB each. */
function logFilesUnder(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((e) => e.isFile() && e.name.endsWith(".log"))
    .map((e) => join(e.parentPath, e.name))
    .filter((f) => statSync(f).size < 20_000_000);
}
function dropRunRoot() {
  saveAppLogs();
  if (KEEP) console.log(`kept: the run's folder ${RUN_ROOT} (CYBERCTF_TEST_DESKTOP_KEEP)`);
  else {
    rmSync(RUN_ROOT, { recursive: true, force: true });
    if (existsSync(RUN_ROOT)) console.log(`couldn't remove all of the run's folder ${RUN_ROOT}`);
  }
}
// What is on the machine before the run: what the run downloads is told apart from it.
recordMachine(RUN_ROOT);
// A signed-in session, for the specs that start catalogue labs: the run's app keeps it in a file
// under the run's folder whatever the build, never the user's file or keychain.
const SESSION = process.env.CYBERCTF_TEST_ACCOUNT_SESSION || "";
if (SESSION) {
  if (!existsSync(SESSION)) die(`no session file: ${SESSION}`);
  const dir = join(FOLDERS.home, ".cyberctf");
  mkdirSync(dir, { recursive: true });
  copyFileSync(SESSION, join(dir, "dev-session.json"));
  if (!WINDOWS) chmodSync(join(dir, "dev-session.json"), 0o600);
  say("session: placed in the run's folder");
}

// --- Fixtures --------------------------------------------------------------------------------
/** The lab folder the local specs import: CYBERCTF_TEST_LOCAL_LAB, else the fixture lab checked
 *  out (the same lab the server tests deploy, under infra/test-servers/.state; on Windows under
 *  tests/desktop/.state), pulled when it is there already. */
function fixtureLab() {
  if (process.env.CYBERCTF_TEST_LOCAL_LAB) return process.env.CYBERCTF_TEST_LOCAL_LAB;
  const dir = join(WINDOWS ? join(HERE, ".state") : join(ROOT, "infra", "test-servers", ".state"), "labs", "invoice-portal-api");
  try {
    if (existsSync(join(dir, ".git"))) execFileSync("git", ["-C", dir, "pull", "-q"], { stdio: "ignore", timeout: 120000 });
    else execFileSync("git", ["clone", "-q", "https://github.com/CyberCTF/invoice-portal-api", dir], { stdio: "ignore", timeout: 300000 });
  } catch (e) {
    console.log(`(the fixture lab couldn't be checked out: ${e.message})`);
  }
  return dir;
}
const LAB = fixtureLab();
process.env.CYBERCTF_TEST_LOCAL_LAB = LAB;
const LAB_OK = existsSync(join(LAB, "isoloom.yml"));
const LAB_NAME = LAB_OK ? (readFileSync(join(LAB, "isoloom.yml"), "utf8").match(/^name:\s*["']?([^"'\n]+)/m) || [])[1]?.trim() : "";
const LAB_ID = LAB_OK ? `local-${localLabName(LAB)}` : "";

// --- The run's own apps and labs --------------------------------------------------------------
/** The process `pid`'s program, when it runs; "" otherwise. */
function programOf(pid) {
  const out = WINDOWS
    ? runTool("powershell.exe", ["-NoProfile", "-Command", `(Get-Process -Id ${pid} -ErrorAction SilentlyContinue).Path`])
    : runTool("ps", ["-p", String(pid), "-o", "command="]);
  return (out ?? "").trim();
}
function killPid(pid) {
  try {
    if (WINDOWS) execFileSync("taskkill", ["/T", "/F", "/PID", String(pid)], { stdio: "ignore" });
    else process.kill(pid, "SIGKILL");
  } catch {
    /* gone already */
  }
}
/** The deploy workers whose job is in the run's folder, stopped; whether any was. */
function stopDeployWorkers() {
  if (WINDOWS) {
    const script = `Get-CimInstance Win32_Process -Filter "Name = '${basename(APP)}'" | Where-Object { $_.CommandLine -and $_.CommandLine.Contains('deploy --job') -and $_.CommandLine.Contains('${RUN_ROOT.replace(/'/g, "''")}') } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force; 'killed' }`;
    return (runTool("powershell.exe", ["-NoProfile", "-Command", script]) ?? "").includes("killed");
  }
  return runTool("pkill", ["-9", "-f", `deploy --job ${RUN_ROOT}`]) !== null;
}
/** The run's own apps still up, stopped, never another copy of the same binary: the ones the run
 *  launched (records/apps/<pid>, by support.mjs and the preflight; fail-signedout's under
 *  signed-out/), while that pid is still the app, and the deploy workers whose job is in the
 *  run's folder. */
function stopRunApps() {
  let killed = false;
  for (const dir of [join(FOLDERS.records, "apps"), join(RUN_ROOT, "signed-out", "records", "apps")]) {
    if (!existsSync(dir)) continue;
    for (const f of readdirSync(dir)) {
      const pid = Number(f);
      if (pid && programOf(pid).includes(WINDOWS ? basename(APP) : APP)) {
        killPid(pid);
        killed = true;
      }
      rmSync(join(dir, f), { force: true });
    }
  }
  if (stopDeployWorkers()) killed = true;
  if (killed) sleepSync(1000);
}

/** The run's own Compose projects (owned.mjs), taken down; the next spec never starts on what is
 *  left. `final` honours CYBERCTF_TEST_DESKTOP_KEEP. Nothing to do while no engine answers. */
function cleanupLab(final = false) {
  if ((final && KEEP) || engineWhy !== null) return;
  for (const p of projects(RUN_ROOT).ours) {
    say(`cleanup: docker compose -p ${p} down (the run's)`);
    runTool("docker", ["compose", "-p", p, "down", "--remove-orphans"]);
  }
  const left = projects(RUN_ROOT).ours;
  if (left.length && !final) die(`the run's Compose projects are left after the clean-up: ${left.join(" ")}`);
}
/** The fixture lab's Compose project when it runs here but isn't the run's (the user's own copy
 *  of the lab, the same name): the cases that import the lab leave it alone. */
const otherCopy = () => (engineWhy === null && LAB_ID && projects(RUN_ROOT).theirs.includes(`cyberctf-${LAB_ID}`) ? `cyberctf-${LAB_ID}` : "");

// --- The app ---------------------------------------------------------------------------------
/** Launches the app once and waits for its embedded WebDriver server (APP_UP_S); null when it
 *  answered, else why not (its output's last lines). */
async function appComesUp(log) {
  const out = [];
  const proc = spawn(APP, [], { env: { ...process.env, TAURI_WEBDRIVER_PORT: String(PORT) }, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  proc.stdout.on("data", (d) => out.push(String(d)));
  proc.stderr.on("data", (d) => out.push(String(d)));
  mkdirSync(join(FOLDERS.records, "apps"), { recursive: true });
  writeFileSync(join(FOLDERS.records, "apps", String(proc.pid)), APP);
  let up = false;
  for (let i = 0; i < APP_UP_S && !up; i++) {
    if (proc.exitCode !== null) break;
    up = await fetch(`http://127.0.0.1:${PORT}/status`).then(
      (r) => r.ok,
      () => false,
    );
    if (!up) await new Promise((r) => setTimeout(r, 1000));
  }
  const exited = proc.exitCode;
  killPid(proc.pid);
  writeFileSync(log, out.join(""));
  stopRunApps();
  if (up) return null;
  const tail = failureTail(out.join(""), 8);
  return exited !== null
    ? `the app exited (code ${exited}) before its WebDriver server answered: ${tail}`
    : `no embedded WebDriver server on ${PORT} within ${APP_UP_S} s: ${tail}`;
}

// --- The needs the runner provides -----------------------------------------------------------
const state = needsState(plan.providers);
let engineWhy = null;
/** The hypervisors support.mjs can ask for their VMs, by their tool; another is taken as given. */
const ASKABLE = { libvirt: "virsh", virtualbox: "VBoxManage", hyperv: "Get-VM", utm: "utmctl" };
let utmVm = "";
const CHECKS = {
  engine: () => {
    try {
      execFileSync("docker", ["info", "--format", "{{.ServerVersion}}"], { stdio: "ignore", timeout: 60000, windowsHide: true });
      return null;
    } catch (e) {
      return `no Docker engine answers here (docker info: ${e.code === "ENOENT" ? "not installed" : (e.message.split("\n")[0] ?? "failed")})`;
    }
  },
  "fixture-lab": () => (LAB_OK ? null : `no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab (${LAB} has no isoloom.yml)`),
  session: () => (SESSION ? null : "no session file: give CYBERCTF_TEST_ACCOUNT_SESSION to the runner (a catalogue lab starts only signed in)"),
  hypervisor: () => {
    if (!PROVIDER) return "no hypervisor named: set CYBERCTF_TEST_VM_PROVIDER (libvirt, virtualbox, hyperv, qemu, utm…)";
    if (PROVIDER in ASKABLE && hypervisorVms(PROVIDER) === null) return `CYBERCTF_TEST_VM_PROVIDER=${PROVIDER}, but ${ASKABLE[PROVIDER]} doesn't answer here`;
    return null;
  },
  server: () => {
    const env = { ...process.env };
    const file = env.CYBERCTF_TEST_SERVER_ENV;
    if (file && existsSync(file))
      for (const line of readFileSync(file, "utf8").split(/\r?\n/))
        if (line.includes("=") && !line.startsWith("#")) env[line.slice(0, line.indexOf("="))] = line.slice(line.indexOf("=") + 1);
    return (env.CYBERCTF_TEST_PVE_HOST && env.CYBERCTF_TEST_PVE_TOKEN_ID) || env.CYBERCTF_TEST_ESXI_HOST
      ? null
      : "no server: set CYBERCTF_TEST_SERVER_ENV to a file with CYBERCTF_TEST_PVE_HOST/_TOKEN_ID/_TOKEN_SECRET or CYBERCTF_TEST_ESXI_HOST/_PASSWORD";
  },
  "own-frame": () => (process.platform === "linux" || WINDOWS ? null : "macOS keeps the system's window frame: the app draws no window controls there"),
  unix: () => (WINDOWS ? "kills the worker with SIGKILL: Unix only (Windows: taskkill /F, the rig's own case)" : null),
  "utm-fixture-vm": () => utmFixtureVm(),
};
/** Why runner-provided `need` isn't met, checked once; null when it is. */
function runnerNeed(need) {
  if (!state.checked(need)) {
    const why = CHECKS[need]();
    state.provided(need, why);
    if (need === "engine") engineWhy = why;
  }
  return state.unmet(need);
}
/** The first unmet need of `c`, the runner's checked as they come up. */
function blockedBy(c) {
  for (const need of c.needs) {
    const why = need in RUNNER_NEEDS ? runnerNeed(need) : state.unmet(need);
    if (why) return why;
  }
  return null;
}

/**
 * UTM on a Mac: a started VM named `<lab> · <machine>` is what the running-labs scan looks for,
 * where `<lab>` is the installed lab's VM-name prefix: its isoloom name plus `-<instance>` when
 * the lab holds an instance number (`.cyberctf-instance`, 1 and up), the plain name otherwise.
 * An empty QEMU VM is enough, it boots to its firmware and shows as started. Made here, dropped
 * after the scan; why it can't be, otherwise.
 */
function utmFixtureVm() {
  if (process.platform !== "darwin")
    return "the UTM fixture is macOS only; elsewhere start a libvirt/Hyper-V VM named for the lab and run tests/desktop/specs/vm-scan.mjs with CYBERCTF_TEST_SCAN_LAB";
  if (!existsSync(UTMCTL)) return "UTM is not installed";
  const installed = join(FOLDERS.labs, LAB_ID);
  if (!LAB_ID || !existsSync(installed)) return `the lab is not installed under ${FOLDERS.labs} (the launch spec imports it)`;
  let prefix = LAB_NAME;
  const instance = existsSync(join(installed, ".cyberctf-instance")) ? readFileSync(join(installed, ".cyberctf-instance"), "utf8").trim() : "";
  if (/^[1-9][0-9]*$/.test(instance)) prefix = `${LAB_NAME}-${instance}`;
  const vm = `${prefix} · dc01`;
  const arch = process.arch === "arm64" ? "aarch64" : "x86_64";
  say(`UTM fixture VM: ${vm}`);
  try {
    utmVm = execFileSync(
      "osascript",
      [
        "-e",
        'tell application "UTM"',
        "-e",
        `set vm to make new virtual machine with properties {backend:qemu, configuration:{name:"${vm}", architecture:"${arch}", memory:256, cpu cores:1}}`,
        "-e",
        "start vm",
        "-e",
        "return id of vm",
        "-e",
        "end tell",
      ],
      { encoding: "utf8", timeout: 60000 },
    ).trim();
  } catch (e) {
    return `could not create the UTM fixture VM (${failureTail(e.stderr ?? e.message, 1)})`;
  }
  for (let i = 0; i < 15 && !(runTool(UTMCTL, ["status", utmVm]) ?? "").includes("started"); i++) sleepSync(2000);
  return null;
}
function dropUtmVm() {
  if (!utmVm) return;
  runTool(UTMCTL, ["stop", "--kill", utmVm]);
  sleepSync(2000);
  runTool(UTMCTL, ["delete", utmVm]);
  utmVm = "";
}

// --- Running a spec --------------------------------------------------------------------------
/** The environment a case adds to its spec's: the lab form, the signed-out folder, the scan's lab. */
function caseEnv(c) {
  const env = { CYBERCTF_TEST_DESKTOP_TIMEOUT: String(capOf(c)) };
  if (c.form) env.CYBERCTF_TEST_LAB_FORM = c.form;
  if (c.case === "fail-signedout") env.CYBERCTF_WDIO_ROOT = join(RUN_ROOT, "signed-out");
  if (c.case === "vm-scan") env.CYBERCTF_TEST_SCAN_LAB = LAB_ID;
  return env;
}
/** `node specs/<spec>.mjs` for case `c`, its output in its log, killed past its cap: its exit
 *  status (null when killed) and its log's text. */
function runSpec(c) {
  const log = join(LOGS, `${c.case}.log`);
  const cap = capOf(c);
  return new Promise((done) => {
    const chunks = [];
    const proc = spawn(process.execPath, [join(args.plan, "specs", `${c.spec}.mjs`)], {
      cwd: ROOT,
      env: { ...process.env, ...caseEnv(c) },
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    proc.stdout.on("data", (d) => chunks.push(String(d)));
    proc.stderr.on("data", (d) => chunks.push(String(d)));
    let killed = false;
    const timer = setTimeout(() => {
      killed = true;
      chunks.push(`\nrun.mjs: ${c.case} exceeded its ${cap}s\n`);
      killPid(proc.pid);
    }, cap * 1000);
    proc.on("exit", (code) => {
      clearTimeout(timer);
      const text = chunks.join("");
      writeFileSync(log, text);
      done({ code: killed ? null : code, text, log, cap });
    });
  });
}

/** One case, in the run: its needs, its spec alone (no app left from before, nothing of the run's
 *  labs up), its verdict and its checks' lines; what the spec printed about the app's death. */
async function runCase(c) {
  const why = blockedBy(c);
  if (why) {
    skipCase(c, why);
    state.verdict(c.case, "skip", why);
    return null;
  }
  const other = c.knobs.includes("CYBERCTF_TEST_LOCAL_LAB") ? otherCopy() : "";
  if (other) {
    skipCase(c, `a copy of ${LAB_NAME} this run didn't start runs here (Compose project ${other}): left as it is`);
    state.verdict(c.case, "skip", "the user's copy of the lab runs here");
    return null;
  }
  stopRunApps();
  cleanupLab();
  say(c.case);
  const start = Date.now();
  const { code, text, log, cap } = await runSpec(c);
  stopRunApps();
  if (c.case === "vm-scan") dropUtmVm();
  const seconds = Math.round((Date.now() - start) / 1000);
  const checks = checkLines(text);
  for (const l of checks) console.log(`   ${{ ok: "ok", FAIL: "FAILED", skip: "skipped" }[l.status]}: ${l.id} (${l.seconds}s) ${l.text}`);
  if (checks.length === 0) for (const l of text.trim().split("\n").slice(-4)) console.log(`   ${l}`);
  const base = { form: c.form, provider: providerOf(c), log };
  let status;
  let detail;
  if (code === 0) [status, detail] = ["ok", log];
  else if (code === 2) [status, detail] = ["skip", skipReason(text) ?? `skipped (see ${log})`];
  else if (code === null) [status, detail] = ["FAIL", `exceeded its ${cap} s: ${failureTail(text)}`];
  else [status, detail] = ["FAIL", `${checks.find((l) => l.status === "FAIL")?.text ?? failureTail(text)} (exit ${code})`];
  record({ name: c.case, status, seconds, detail, ...base });
  for (const l of checks)
    if (l.id !== "spec") record({ name: `${c.case}/${l.id}`, status: l.status, seconds: l.seconds, detail: l.text, ...base, counted: false });
  state.verdict(c.case, status, detail);
  return status === "FAIL" && DEAD_APP.test(text) ? failureTail(text) : null;
}

// --- The run -----------------------------------------------------------------------------------
say(`app: ${APP}`);
say(`lab: ${LAB}${LAB_OK ? ` (${LAB_NAME}, id ${LAB_ID})` : " (not a lab folder)"}`);
const preflight = await appComesUp(join(LOGS, "preflight.log"));
if (preflight !== null) {
  dropRunRoot();
  die(`${preflight}\nthe app must be built with --features wdio`);
}

let ended = null;
let restarted = false;
const queue = cases.filter((c) => wanted(c.case));
for (let i = 0; i < queue.length; i++) {
  const c = queue[i];
  if (ended) {
    skipCase(c, ended);
    continue;
  }
  const death = await runCase(c);
  if (death === null) continue;
  // The app died under the spec. Once, it is started again to see whether it comes up at all;
  // a second death, or none, ends the run: every case left would die the same way.
  if (restarted) {
    ended = `the app died again (${death}): the run ended`;
    continue;
  }
  restarted = true;
  say(`${c.case}: the app died (${death}); starting it again once`);
  const again = await appComesUp(join(LOGS, "restart.log"));
  if (again !== null) ended = `the app died (${death}) and didn't come up again: ${again}`;
}

stopRunApps();
dropUtmVm();
cleanupLab(true);
dropRunRoot();
say(`desktop e2e, logs in ${LOGS}`);
for (const l of results.lines) console.log(`   ${l}`);
console.log(`\n${results.failed} failed, ${results.skipped} skipped`);
if (results.failed > 0) process.exit(1);
if (process.env.STRICT && results.skipped > 0) process.exit(1);
process.exit(0);
