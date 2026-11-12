// What the desktop end-to-end's runner and specs read of this machine. The app's WebDriver port:
// whether it is free to bind, what holds it, and a wait for it to be freed. One app answers on it
// at a time (TAURI_WEBDRIVER_PORT); an app that can't bind it runs on without its server, so a
// spec would drive whatever still answers there (an earlier app, another copy of the app) without
// the knobs it started its own with. The runner (run.mjs) and the specs' launches (support.mjs)
// wait for it before each app instead, and stop with what holds it. And where the Docker CLI finds
// its plugins (Compose), which a spec's Docker config of its own must keep. And a program run with
// a bound, ended with what it started past it (`runBounded`), and whether `docker info` says an
// engine answers. No dependency of its own, so the runner loads it before webdriverio.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { createServer } from "node:net";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

/** The supervisor `runBounded` runs a program under (resolved when asked: a page's test loads this
 *  module in jsdom, where the module's URL isn't a file's). */
const bounded = () => fileURLToPath(new URL("./bounded.mjs", import.meta.url));

/**
 * `program args`, without a shell, stdin the null device, for at most `timeoutMs`: past it, the
 * program is ended with everything it started (lib/bounded.mjs: taskkill /T on Windows, each
 * process under it elsewhere), so neither it nor a helper of its (a Docker CLI plugin) outlives
 * the call or holds its output open. `{ out, code, timedOut, missing }`: its output, its exit code
 * (null when it was ended or never ran), whether it ran past the bound, whether it isn't installed.
 * @param {string} program
 * @param {string[]} args
 * @param {number} timeoutMs
 * @returns {{ out: string, code: number | null, timedOut: boolean, missing: boolean }}
 */
export function runBounded(program, args, timeoutMs) {
  const r = spawnSync(process.execPath, [bounded(), String(timeoutMs), program, ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
    // The supervisor's own bound, should it hang: its answer is then a time-out.
    timeout: timeoutMs + 20000,
    killSignal: "SIGKILL",
    windowsHide: true,
  });
  try {
    return JSON.parse(r.stdout);
  } catch {
    return { out: "", code: null, timedOut: true, missing: false };
  }
}

/**
 * Why `docker info --format …` (`runBounded`'s answer, bounded at `seconds`) says no Docker engine
 * answers here; null when one does, naming something. Docker 26 and older exit 0 with an empty
 * line when the daemon doesn't answer (#291), so an exit 0 alone isn't an answer.
 * @param {{ out: string, code: number | null, timedOut: boolean, missing: boolean }} info
 * @param {number} seconds
 * @returns {string | null}
 */
export function engineSilence({ out, code, timedOut, missing }, seconds) {
  if (code === 0 && out.trim() !== "") return null;
  if (missing) return "not installed";
  if (timedOut) return `no answer in ${seconds} s`;
  if (code === 0) return "an empty answer: its daemon doesn't answer";
  return code === null ? "ended" : `exit ${code}`;
}

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

/**
 * The folders the Docker CLI finds its plugins in through its config folder, which a
 * `DOCKER_CONFIG` of a spec's own hides: `<config>/cli-plugins` (`$DOCKER_CONFIG`, else
 * `~/.docker`) and the config's own `cliPluginsExtraDirs`, the ones there. Docker Desktop on macOS
 * keeps Compose there alone; without it `docker compose --project-name x ps` is the Docker CLI's
 * `unknown flag: --project-name`, a missing Compose rather than the engine the spec stops.
 * @param {Record<string, string | undefined>} env
 * @param {string} home
 * @returns {string[]}
 */
export function dockerPluginDirs(env = process.env, home = homedir()) {
  const config = env.DOCKER_CONFIG || (home ? join(home, ".docker") : "");
  if (!config) return [];
  let extra = [];
  try {
    const listed = JSON.parse(readFileSync(join(config, "config.json"), "utf8")).cliPluginsExtraDirs;
    if (Array.isArray(listed)) extra = listed.filter((d) => typeof d === "string");
  } catch {
    /* no config, or one that doesn't read: its plugin folder alone */
  }
  return [...new Set([join(config, "cli-plugins"), ...extra])].filter((d) => existsSync(d));
}
