// Shared helpers for the native GUI e2e: launch the real app (built with `--features wdio`) so its
// embedded WebDriver server comes up, connect a WebdriverIO session to it, drive the shell, and
// tear everything down. macOS, Linux and Windows. Every script goes through here, so every script
// honours APP_BIN (or CYBERCTF_BIN) and TAURI_WEBDRIVER_PORT; e2e/desktop/run.sh sets both.
import { remote } from "webdriverio";
import { execFileSync, spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

/** Path to the app binary: APP_BIN, else CYBERCTF_BIN, else the debug build. */
export function appBinary() {
  const given = process.env.APP_BIN || process.env.CYBERCTF_BIN;
  if (given) return given;
  const ext = process.platform === "win32" ? ".exe" : "";
  return fileURLToPath(new URL(`../../src-tauri/target/debug/cyberctf-desktop${ext}`, import.meta.url));
}

/** The port the embedded WebDriver server listens on (override with TAURI_WEBDRIVER_PORT). */
export const PORT = Number(process.env.TAURI_WEBDRIVER_PORT || 4445);

/**
 * An environment in which the app cannot reach any Docker engine: a dead `DOCKER_HOST` and a
 * `DOCKER_CONFIG` whose config.json names no current context. The second half matters: the app
 * deliberately follows the user's Docker context (`docker context use`, what Docker Desktop sets)
 * over an inherited `DOCKER_HOST`, so on a machine with a context the dead socket alone is ignored
 * and the real engine answers. Fresh folder per call.
 */
export function deadDockerEnv() {
  const dir = mkdtempSync(join(tmpdir(), "cyberctf-nodocker-"));
  writeFileSync(join(dir, "config.json"), "{}\n");
  return { DOCKER_HOST: "unix:///nonexistent/docker.sock", DOCKER_CONFIG: dir };
}

/** A lab shows as running on the Overview. */
export const LAB_RUNNING = /\blab[s]? running on this machine\b/i;

/** Every app this process launched, killed when a spec skips mid-way (`results().skipAll`). */
const launched = new Set();

/**
 * Launch the app and wait for its embedded WebDriver server to accept connections. `env` is added
 * to the app's environment (a seed lab, a dead Docker socket, a dead backend), `args` to its
 * command line (a `cyberctf://` link, as the system opens one). The returned child carries
 * `output()`, the app's stdout and stderr so far, for a failure report.
 */
export async function launchApp({ env = {}, args = [] } = {}) {
  const bin = appBinary();
  if (!existsSync(bin)) throw new Error(`app binary not found (build with --features wdio): ${bin}`);
  const proc = spawn(bin, args, {
    env: { ...process.env, TAURI_WEBDRIVER_PORT: String(PORT), ...env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  launched.add(proc);
  let out = "";
  proc.stdout.on("data", (d) => (out += d));
  proc.stderr.on("data", (d) => (out += d));
  proc.output = () => out;
  for (let i = 0; i < 60; i++) {
    if (proc.exitCode !== null) throw new Error(`app exited (code ${proc.exitCode}) before the server came up:\n${out.slice(-800)}`);
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/status`);
      if (r.ok) return proc;
    } catch {
      /* not up yet */
    }
    await sleep(1000);
  }
  stopApp(proc);
  throw new Error(`embedded WebDriver server never came up on ${PORT}:\n${out.slice(-800)}`);
}

/** Connect a WebdriverIO session to the running app's embedded server. */
export async function connect() {
  return remote({
    hostname: "127.0.0.1",
    port: PORT,
    path: "/",
    logLevel: "error",
    capabilities: { browserName: "wry", "wdio:enforceWebDriverClassic": true },
  });
}

/**
 * Skip onboarding, pin English, add `storage` (the `__wdio_*` hooks), reload, and wait for the
 * shell's navigation. The locale is pinned because the scripts match English copy.
 */
export async function prepare(b, storage = {}) {
  await b.execute((items) => {
    try {
      // The webview's storage outlives the app: a hook an earlier scenario set (the attack-box
      // panel's lab, say) would otherwise mount again in this one, in every window.
      for (const k of Object.keys(localStorage)) if (k.startsWith("__wdio_")) localStorage.removeItem(k);
      localStorage.setItem("cyberctf.onboarded", "1");
      localStorage.setItem("cyberctf.locale", "en");
      for (const [k, v] of Object.entries(items)) localStorage.setItem(k, v);
    } catch {
      /* storage unavailable: the shell still renders */
    }
  }, storage);
  await b.refresh();
  await b.$("aside nav").waitForExist({ timeout: 25000 });
  await forceVisible(b);
}

/**
 * Make the page believe its window is visible, and say whether that was needed. The status
 * polls pause while the document is hidden (src/lib/visibility.ts), and a window another one
 * covers, or a headless one, reports hidden on macOS, so without this the Overview never learns
 * that a lab came up. A reload (the app's own, or the dev server's) drops it, so `waitForLab`
 * calls this on every tick.
 */
export async function forceVisible(b) {
  return b.execute(() => {
    try {
      if (window.__wdioVisible && document.visibilityState === "visible") return false;
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
      window.__wdioVisible = true;
      document.dispatchEvent(new Event("visibilitychange"));
      window.dispatchEvent(new Event("focus"));
      return true;
    } catch {
      return false;
    }
  });
}

/** Launch, connect and prepare in one go; the app is stopped again if the session can't open. */
export async function openApp({ env = {}, storage = {}, args = [] } = {}) {
  const proc = await launchApp({ env, args });
  try {
    const b = await connect();
    await prepare(b, storage);
    return { proc, b };
  } catch (e) {
    stopApp(proc);
    throw e;
  }
}

/** Click a sidebar tab by its label (`Overview`, `Labs`, `Machine`). */
export async function goTo(b, tab) {
  await (await b.$("aside nav").$(`button*=${tab}`)).click();
}

/** The main pane's text, empty when it can't be read. */
export const mainText = (b) =>
  b
    .$("main")
    .getText()
    .catch(() => "");

/** Whether the Overview says a lab runs on this machine (and not "Nothing running"). */
export async function labRunning(b) {
  const t = await mainText(b);
  return LAB_RUNNING.test(t) && !/Nothing running/i.test(t);
}

/** `b.waitUntil(cond)` that keeps the page's polls awake: the visibility override is re-applied on
 *  every tick, since a reload drops it and a covered window reports hidden again. */
export function waitAwake(b, cond, { timeout, interval = 4000, timeoutMsg }) {
  let ticks = 0;
  return b.waitUntil(
    async () => {
      if ((await forceVisible(b)) && ticks > 0) console.log("visibility override re-applied (the page reloaded)");
      ticks += 1;
      return cond();
    },
    { timeout, interval, timeoutMsg },
  );
}

/** Wait until a lab shows as running on the Overview. */
export function waitForLab(b, timeout = 220000, timeoutMsg = "lab not shown running") {
  return waitAwake(b, () => labRunning(b), { timeout, timeoutMsg });
}

/**
 * The slug a local lab folder is imported under (`local-<slug>`, see `labs::local_id`): the
 * lab's isoloom name, lower-cased, anything but `[a-z0-9_-]` turned into `-`.
 */
export function localLabName(lab) {
  const yaml = readFileSync(`${lab}/isoloom.yml`, "utf8");
  const name = (yaml.match(/^name:\s*["']?([^"'\n]+)/m) || [])[1] || lab.split(/[\\/]/).filter(Boolean).pop();
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 56);
}

/**
 * Answers the app's destructive confirmations (`confirmDestructive`, src/lib/confirm.ts) from now
 * on, in this window, with their confirm button (`answer` "cancel": with Cancel): they open the
 * system's own dialog, which WebDriver can't press, so the webview's dialog call is answered
 * before it reaches the core. Called again, it answers the new way.
 */
export async function confirmSystemDialogs(b, answer = "confirm") {
  await b.execute((a) => {
    const internals = window.__TAURI_INTERNALS__;
    internals.__answer = a;
    if (internals.__confirming) return;
    const invoke = internals.invoke.bind(internals);
    internals.invoke = (cmd, args, options) =>
      cmd === "plugin:dialog|message" && args?.buttons?.OkCancelCustom
        ? Promise.resolve(args.buttons.OkCancelCustom[internals.__answer === "cancel" ? 1 : 0])
        : invoke(cmd, args, options);
    internals.__confirming = true;
  }, answer);
}

/**
 * Invoke a Tauri command directly, the way the UI does, and resolve with its result. Needed for
 * paths with no local-lab UI button (park/resume, re-import): the webview's `invoke` returns a
 * promise, which the classic wry WebDriver cannot await, so the call parks its outcome on a window
 * global that we poll. `streaming` adds the `logs: __CHANNEL__:<cb>` argument that the streaming
 * commands (lab_stop/park/resume/launch…) require; a plain command (lab_status) omits it.
 */
export async function invokeCmd(b, cmd, args = {}, { streaming = false, timeoutMs = 150000 } = {}) {
  await b.execute(
    (c, a, stream) => {
      window.__inv = "pending";
      const internals = window.__TAURI_INTERNALS__;
      const full = stream ? { ...a, logs: `__CHANNEL__:${internals.transformCallback(() => {})}` } : a;
      internals.invoke(c, full).then(
        (v) => (window.__inv = { ok: v === undefined ? null : v }),
        (e) => {
          let m;
          try {
            m = typeof e === "string" ? e : e && e.message ? e.message : JSON.stringify(e);
          } catch {
            m = String(e);
          }
          window.__inv = { err: m };
        },
      );
    },
    cmd,
    args,
    streaming,
  );
  const deadline = Date.now() + timeoutMs;
  let r = "pending";
  while (r === "pending" && Date.now() < deadline) {
    await sleep(1000);
    r = await b.execute(() => window.__inv).catch(() => "pending");
  }
  if (r === "pending") throw new Error(`${cmd}: timed out after ${timeoutMs}ms`);
  if (r && r.err) throw new Error(`${cmd}: ${r.err}`);
  return r ? r.ok : null;
}

/**
 * Invoke a streaming lab command, retrying while the per-lab lock is still held by another
 * operation (the "another operation … is already in progress" refusal): a launch holds the lock
 * briefly after the lab already shows running, and the real UI gates its buttons on that, so a
 * direct invoke waits the same way instead of racing it.
 */
export async function invokeWhenIdle(b, cmd, args, timeoutMs = 240000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      return await invokeCmd(b, cmd, args, { streaming: true });
    } catch (e) {
      if (Date.now() < deadline && /already in progress/i.test(e.message)) {
        await sleep(2000);
        continue;
      }
      throw e;
    }
  }
}

/** Wait until lab `id` has no operation in flight (its launch worker has finished), so a
 *  lifecycle command won't race the launch for the per-lab lock. Polls the app's own `busy_labs`;
 *  a failed read is tried again, never taken for "settled", and a lab still busy at the deadline
 *  fails the scenario. */
export async function waitSettled(b, id, timeoutMs = 240000) {
  const deadline = Date.now() + timeoutMs;
  let failure = null;
  while (Date.now() < deadline) {
    let busy;
    try {
      busy = await invokeCmd(b, "busy_labs");
      failure = null;
    } catch (e) {
      failure = e;
      await sleep(2000);
      continue;
    }
    if (!Array.isArray(busy)) throw new Error(`busy_labs answered ${JSON.stringify(busy)}, not a list of labs`);
    if (!busy.includes(id)) return;
    await sleep(2000);
  }
  const why = failure ? ` (busy_labs last failed: ${failure instanceof Error ? failure.message : String(failure)})` : "";
  throw new Error(`${id} still had an operation in flight after ${String(timeoutMs / 1000)} s${why}`);
}

/** Stop the app process. */
export function stopApp(proc) {
  if (proc && proc.exitCode === null) {
    try {
      proc.kill("SIGKILL");
    } catch {
      /* already gone */
    }
  }
}

/** End the session and stop the app; safe when either never came up. */
export async function finish(b, proc) {
  if (b) await b.deleteSession().catch(() => {});
  stopApp(proc);
}

export { sleep };

// --- Results ---------------------------------------------------------------------------------
// A spec's checks, one line each, as the integration tests print theirs (`ok:` / `FAILED:`, see
// src-tauri/src/integration_tests/support.rs), with a stable id and the seconds since the line
// before, so the runner can put a line per check in its summary:
//
//   ok: <id> (<seconds>s) <what was seen>
//   FAILED: <id> (<seconds>s) <what was seen>
//   skipped: <id> (0s) <why>
//
// The spec's exit status is its verdict: 0 every check passed (or was skipped), 1 one failed,
// 2 the whole spec was skipped (`skipAll`), a knob it needs missing.

/** Exit status of a spec skipped as a whole. */
export const SKIPPED = 2;

/** The result lines of one spec. */
export function results() {
  let failed = 0;
  let mark = Date.now();
  const line = (status, id, text) => {
    const secs = Math.round((Date.now() - mark) / 1000);
    mark = Date.now();
    console.log(`${status}: ${id} (${status === "skipped" ? 0 : secs}s) ${String(text).replace(/\s+/g, " ").trim()}`);
  };
  const r = {
    pass: (id, what) => line("ok", id, what),
    fail: (id, what) => {
      failed += 1;
      line("FAILED", id, what);
    },
    /** A check: `ok` decides which line; returns `ok`. */
    check: (id, ok, what) => {
      if (ok) r.pass(id, what);
      else r.fail(id, what);
      return ok;
    },
    skip: (id, why) => line("skipped", id, why),
    /** Runs `fn`; its answer (or `what`) is the ok line, what it throws the FAILED one. */
    step: async (id, what, fn) => {
      try {
        const seen = await fn();
        r.pass(id, typeof seen === "string" && seen ? seen : what);
        return true;
      } catch (e) {
        r.fail(id, `${what}: ${e?.message ?? e}`);
        return false;
      }
    },
    /** The whole spec skipped: one line, the apps it launched stopped, and exit. */
    skipAll: (why) => {
      line("skipped", "spec", why);
      for (const p of launched) stopApp(p);
      process.exit(SKIPPED);
    },
    get failed() {
      return failed;
    },
    /** Exit with the verdict. */
    exit: () => process.exit(failed > 0 ? 1 : 0),
  };
  return r;
}

// --- Knobs and the machine --------------------------------------------------------------------

/** The lab folder the local scenarios import, when it is one. */
export function localLab() {
  const lab = process.env.CYBERCTF_TEST_LOCAL_LAB;
  return lab && existsSync(join(lab, "isoloom.yml")) ? lab : null;
}

/** The catalogue lab the signed-in scenarios start (its title in the Labs list). */
export const CATALOGUE_LAB = process.env.CYBERCTF_TEST_CATALOGUE_LAB || "Supplier portal API";

/** `docker <args>`, without a shell (so `{{.Names}}` and `^` reach Docker as written on every
 *  system); its output, or null when Docker failed. */
export function docker(...args) {
  try {
    return execFileSync("docker", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 60000 });
  } catch {
    return null;
  }
}

/** Whether a Docker engine answers here. */
export const dockerUp = () => docker("info", "--format", "{{.ServerVersion}}") !== null;

/** The containers of lab `id` (its Compose project, `cyberctf-<id>`): name and state each. */
export function labContainers(id) {
  const out = docker("ps", "-a", "--format", "{{.Names}}\t{{.State}}", "--filter", `name=^cyberctf-${id}`) ?? "";
  return out
    .split("\n")
    .filter(Boolean)
    .map((l) => {
      const [name, state] = l.split("\t");
      return { name, state };
    });
}

/** The VMs a hypervisor lists here, by name; null when this one can't be asked. */
export function hypervisorVms(provider) {
  const run = (cmd, args) => {
    try {
      return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 60000 });
    } catch {
      return null;
    }
  };
  const lines = (out) =>
    out === null
      ? null
      : out
          .split(/\r?\n/)
          .map((l) => l.trim())
          .filter(Boolean);
  switch (provider) {
    case "libvirt":
    case "qemu":
      return lines(run("virsh", ["-c", "qemu:///system", "list", "--all", "--name"]));
    case "virtualbox":
      return lines(run("VBoxManage", ["list", "vms"]))?.map((l) => l.replace(/^"(.*)".*$/, "$1")) ?? null;
    case "hyperv":
      return lines(run("powershell.exe", ["-NoProfile", "-Command", "Get-VM | ForEach-Object { $_.Name }"]));
    case "utm":
      return lines(run("/Applications/UTM.app/Contents/MacOS/utmctl", ["list"]))?.slice(1) ?? null;
    default:
      return null;
  }
}

/** The app's data folder, where it installs labs (`<data>/labs/<id>`), as Tauri resolves it for
 *  the identifier `org.cyberctf.desktop`. */
export function appDataDir() {
  const id = "org.cyberctf.desktop";
  if (process.platform === "darwin") return join(homedir(), "Library", "Application Support", id);
  if (process.platform === "win32") return join(process.env.APPDATA || join(homedir(), "AppData", "Roaming"), id);
  return join(process.env.XDG_DATA_HOME || join(homedir(), ".local", "share"), id);
}

/** Whether the app has its own window frame (Linux and Windows), where the page draws the window
 *  controls; macOS keeps the system's. */
export const ownFrame = process.platform === "linux" || process.platform === "win32";

// --- The app's state, asked through its own commands -----------------------------------------

/** Whether the app is signed in to Cyber CTF (`auth_status`). */
export async function signedIn(b) {
  const status = await invokeCmd(b, "auth_status", {}, { timeoutMs: 30000 }).catch(() => null);
  return !!status?.loggedIn;
}

/** The labs imported from a folder installed here (`imported_labs`): id and title each. */
export async function importedLabs(b) {
  const list = await invokeCmd(b, "imported_labs", {}, { timeoutMs: 30000 }).catch(() => []);
  return (Array.isArray(list) ? list : []).map((l) => ({ id: l.id, title: l.metadata?.title ?? l.name }));
}

// --- System dialogs and windows ---------------------------------------------------------------

/**
 * Answers the app's folder and file pickers (`open` of the dialog plugin) with `path` from now
 * on, in this window: the system's picker is a window WebDriver can't drive, so the webview's
 * call is answered before it reaches the core, as `confirmSystemDialogs` answers confirmations.
 * Called again, it answers with the new path.
 */
export async function answerPickers(b, path) {
  await b.execute((p) => {
    const internals = window.__TAURI_INTERNALS__;
    internals.__pickPath = p;
    if (internals.__picking) return;
    const invoke = internals.invoke.bind(internals);
    internals.invoke = (cmd, args, options) => (cmd === "plugin:dialog|open" ? Promise.resolve(internals.__pickPath) : invoke(cmd, args, options));
    internals.__picking = true;
  }, path);
}

/** Waits for the window labelled `label` (`settings`, `machine-setup`, `server-setup`,
 *  `shell-<lab id>`) to exist, and switches the session to it. */
export async function switchToWindow(b, label, timeout = 30000) {
  await b.waitUntil(async () => (await b.getWindowHandles()).includes(label), {
    timeout,
    interval: 500,
    timeoutMsg: `no window "${label}" opened (windows: ${(await b.getWindowHandles().catch(() => [])).join(", ")})`,
  });
  await b.switchToWindow(label);
  await b.$("body").waitForExist({ timeout: 20000 });
  await forceVisible(b);
}

/** Closes window `label` if it is open, and goes back to the main window. */
export async function closeWindow(b, label) {
  if ((await b.getWindowHandles().catch(() => [])).includes(label)) {
    await b.switchToWindow(label).catch(() => {});
    await b.closeWindow().catch(() => {});
  }
  await b.switchToWindow("main").catch(() => {});
}

/** A button in the main pane named exactly one of `labels` (its text, trimmed). */
export const mainButton = (b, ...labels) => b.$(`//main//button[${labels.map((l) => `normalize-space()=${JSON.stringify(l)}`).join(" or ")}]`);

/** A radio row (`role="radio"`) under `scope` whose text holds `text`. */
export const radioRow = (scope, text) => scope.$(`.//*[@role="radio"][contains(normalize-space(), ${JSON.stringify(text)})]`);

// --- Lab pages --------------------------------------------------------------------------------

/** Opens the lab titled `title` from the Labs screen (the search narrows the list first);
 *  false when the list has no such lab. */
export async function openLabFromList(b, title) {
  await goTo(b, "Labs");
  const search = await b.$('main input[aria-label="Search labs and skills"]');
  if (await search.waitForExist({ timeout: 20000 }).catch(() => false)) await search.setValue(title);
  const open = await b.$(`main button[aria-label=${JSON.stringify(`Open ${title}`)}]`);
  if (!(await open.waitForClickable({ timeout: 30000 }).catch(() => false))) return false;
  await open.click();
  await b.waitUntil(
    async () =>
      (
        await b
          .$("main h1")
          .getText()
          .catch(() => "")
      ).includes(title),
    {
      timeout: 30000,
      timeoutMsg: `the page of ${title} never opened`,
    },
  );
  return true;
}

/** Whether the lab page says the lab runs: its header offers Stop & remove, enabled (nothing in
 *  flight: a start, a stop, a park), and no Resume (a parked lab offers both). Throws the page's
 *  failure when its start failed. */
export async function labPageRunning(b) {
  const t = await mainText(b);
  if (/Couldn.t start/.test(t)) throw new Error(`the start failed: ${t.slice(0, 1200)}`);
  const remove = await mainButton(b, "Stop & remove");
  return (await remove.isExisting()) && (await remove.isEnabled()) && !(await (await mainButton(b, "Resume")).isExisting());
}

/** Waits until the lab page shows the lab running. */
export function waitLabPageRunning(b, timeout) {
  return waitAwake(b, () => labPageRunning(b), { timeout, interval: 5000, timeoutMsg: "the lab never showed as running on its page" });
}

/** Stop & remove from the lab page, its system confirmation answered yes, then waits until the
 *  page offers a start again (Start lab, or Import a lab for an imported one). */
export async function stopAndRemoveOnPage(b, timeout = 600000) {
  const stop = await mainButton(b, "Stop & remove", "Stop & clean up");
  await stop.waitForClickable({ timeout: 60000 });
  await confirmSystemDialogs(b);
  await stop.click();
  await waitAwake(b, () => mainButton(b, "Start lab", "Retry", "Import a lab").isExisting(), {
    timeout,
    timeoutMsg: "the lab never stopped",
  });
}

/** The provider names the hypervisor rows show (src/lib/tauri/generated/ProviderNames.ts). */
const PROVIDER_LABELS = {
  utm: "UTM",
  qemu: "QEMU",
  virtualbox: "VirtualBox",
  vmware_desktop: "VMware",
  parallels: "Parallels",
  libvirt: "libvirt",
  hyperv: "Hyper-V",
};

/**
 * Start lab, then the "Where should the lab run?" dialog answered as asked: `place` is
 * `container`, `vm` (`form` `one` or `per-machine`, on `provider`) or `host` (the saved server
 * named `host`); an engine step takes the default engine, a ports step random ports. Returns
 * what the dialog showed, for the log. Throws, saying why, when the place is refused here.
 */
export async function startFromDialog(b, { place, form = "one", provider = null, host = null }) {
  const start = await mainButton(b, "Start lab", "Retry");
  await start.waitForClickable({ timeout: 60000 });
  await start.click();
  const dialog = await b.$('[role="dialog"][aria-label="Where should the lab run?"]');
  await dialog.waitForExist({ timeout: 20000 });
  const pick = async (text) => {
    const row = await radioRow(dialog, text);
    if (!(await row.waitForExist({ timeout: 20000 }).catch(() => false))) throw new Error(`the dialog offers no "${text}":\n${await dialog.getText()}`);
    if ((await row.getAttribute("aria-disabled")) === "true") throw new Error(`"${text}" is refused here: ${(await row.getText()).replace(/\s+/g, " ")}`);
    await row.click();
    await sleep(800);
  };
  const seen = [];
  await pick(place === "container" ? "Container" : place === "vm" ? "VM" : host);
  seen.push(place === "host" ? host : place);
  const answered = new Set();
  for (let i = 0; i < 6; i++) {
    const text = await dialog.getText();
    if (/How should it run\?/.test(text) && !answered.has("form")) {
      answered.add("form");
      await pick(form === "per-machine" ? "One VM per machine" : "In one VM");
      seen.push(form);
    } else if (/Which hypervisor\?/.test(text) && !answered.has("hypervisor")) {
      answered.add("hypervisor");
      if (!provider) throw new Error("the dialog asks which hypervisor: set CYBERCTF_TEST_VM_PROVIDER");
      await pick(PROVIDER_LABELS[provider] ?? provider);
      seen.push(provider);
    } else if (/Which engine\?/.test(text) && !answered.has("engine")) {
      answered.add("engine");
      const current = await dialog.$('.//*[@role="radio"][@aria-checked="true"]');
      await ((await current.isExisting()) ? current : await dialog.$('.//*[@role="radio"]')).click();
      await sleep(800);
      seen.push("default engine");
    } else if (/Which ports should/.test(text) && !answered.has("ports")) {
      answered.add("ports");
      await pick("Random ports");
      seen.push("random ports");
    } else break;
  }
  await (await dialog.$('.//button[normalize-space()="Start"]')).click();
  return seen.join(" > ");
}
