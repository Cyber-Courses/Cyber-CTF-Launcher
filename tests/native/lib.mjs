// Shared helpers for the native GUI e2e: launch the real app (built with `--features wdio`) so its
// embedded WebDriver server comes up, connect a WebdriverIO session to it, drive the shell, and
// tear everything down. macOS, Linux and Windows. Every script goes through here, so every script
// honours APP_BIN (or CYBERCTF_BIN) and TAURI_WEBDRIVER_PORT; tests/native/run-all.sh sets both.
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { existsSync, readFileSync } from "node:fs";

/** Path to the app binary: APP_BIN, else CYBERCTF_BIN, else the debug build. */
export function appBinary() {
  const given = process.env.APP_BIN || process.env.CYBERCTF_BIN;
  if (given) return given;
  const ext = process.platform === "win32" ? ".exe" : "";
  return fileURLToPath(new URL(`../../src-tauri/target/debug/cyberctf-desktop${ext}`, import.meta.url));
}

/** The port the embedded WebDriver server listens on (override with TAURI_WEBDRIVER_PORT). */
export const PORT = Number(process.env.TAURI_WEBDRIVER_PORT || 4445);

/** A lab shows as running on the Overview. */
export const LAB_RUNNING = /\blab[s]? running on this machine\b/i;

/**
 * Launch the app and wait for its embedded WebDriver server to accept connections. `env` is added
 * to the app's environment (a seed lab, a dead Docker socket, a dead backend). The returned child
 * carries `output()`, the app's stdout and stderr so far, for a failure report.
 */
export async function launchApp({ env = {} } = {}) {
  const bin = appBinary();
  if (!existsSync(bin)) throw new Error(`app binary not found (build with --features wdio): ${bin}`);
  const proc = spawn(bin, [], {
    env: { ...process.env, TAURI_WEBDRIVER_PORT: String(PORT), ...env },
    stdio: ["ignore", "pipe", "pipe"],
  });
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
export async function openApp({ env = {}, storage = {} } = {}) {
  const proc = await launchApp({ env });
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
export const mainText = (b) => b.$("main").getText().catch(() => "");

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
