// Shared helpers for the native GUI e2e: launch the real app so its embedded WebDriver server
// comes up, connect a WebdriverIO session to it, and tear down. Works on macOS, Linux and Windows.
// The app must be built with `--features wdio`.
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";

/** Path to the debug app binary (override with APP_BIN). */
export function appBinary() {
  if (process.env.APP_BIN) return process.env.APP_BIN;
  const ext = process.platform === "win32" ? ".exe" : "";
  return fileURLToPath(new URL(`../src-tauri/target/debug/cyberctf-desktop${ext}`, import.meta.url));
}

/** The port the embedded WebDriver server listens on (override with TAURI_WEBDRIVER_PORT). */
export const PORT = Number(process.env.TAURI_WEBDRIVER_PORT || 4445);

/** Launch the app and wait for its embedded WebDriver server to accept connections. */
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
