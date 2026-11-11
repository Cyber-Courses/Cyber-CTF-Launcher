// The app's WebDriver port on this machine, as the desktop end-to-end's runner and specs see it:
// whether it is free to bind, what holds it, and a wait for it to be freed. One app answers on it
// at a time (TAURI_WEBDRIVER_PORT); an app that can't bind it runs on without its server, so a
// spec would drive whatever still answers there (an earlier app, another copy of the app) without
// the knobs it started its own with. The runner (run.mjs) and the specs' launches (support.mjs)
// wait for it before each app instead, and stop with what holds it. No dependency of its own, so
// the runner loads it before webdriverio.
import { execFileSync } from "node:child_process";
import { createServer } from "node:net";
import { setTimeout as sleep } from "node:timers/promises";

/** How long the port may stay held after an app is ended, before the next app (seconds). */
export const PORT_FREE_S = 10;

/** Whether `port` on 127.0.0.1 (where the app's server listens) can be bound now. */
export function portFree(port) {
  return new Promise((done) => {
    const probe = createServer();
    probe.once("error", () => done(false));
    probe.listen({ port, host: "127.0.0.1", exclusive: true }, () => probe.close(() => done(true)));
  });
}

/** Waits until `port` can be bound, polling, at most `seconds`; whether it could. */
export async function waitPortFree(port, seconds = PORT_FREE_S) {
  const deadline = Date.now() + seconds * 1000;
  for (;;) {
    if (await portFree(port)) return true;
    if (Date.now() >= deadline) return false;
    await sleep(250);
  }
}

/** What listens on `port`, as `pid <pid>: <its command>`, when the system says; "" otherwise. */
export function portHolder(port, platform = process.platform) {
  const run = (program, args) => {
    try {
      return execFileSync(program, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 10000, windowsHide: true });
    } catch {
      return "";
    }
  };
  const pid =
    platform === "win32"
      ? run("powershell.exe", [
          "-NoProfile",
          "-Command",
          `(Get-NetTCPConnection -LocalPort ${port} -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1).OwningProcess`,
        ]).trim()
      : (run("lsof", ["-nP", `-iTCP:${port}`, "-sTCP:LISTEN", "-Fp"]).match(/^p(\d+)$/m) ?? [])[1];
  if (!pid || !/^\d+$/.test(pid)) return "";
  const command =
    platform === "win32"
      ? run("powershell.exe", ["-NoProfile", "-Command", `(Get-Process -Id ${pid} -ErrorAction SilentlyContinue).Path`])
      : run("ps", ["-p", pid, "-o", "command="]);
  return `pid ${pid}${command.trim() ? `: ${command.trim()}` : ""}`;
}

/** Why the next app can't be started: `port` is still held after the wait; who holds it, when
 *  the system says. */
export const portStillHeld = (port, holder = portHolder(port)) => `the previous app still holds port ${port}${holder ? ` (${holder})` : ""}`;
