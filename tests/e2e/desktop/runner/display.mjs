// The display the app opens its window on. On a Linux machine without one (DISPLAY and
// WAYLAND_DISPLAY empty: an SSH session, the test rig), GTK can't start and the app panics before
// its WebDriver server answers; the runner starts a virtual one for the run instead (Xvfb, on a
// display number no other X server holds), exports DISPLAY to the specs and their apps, and stops
// it on every way out of the run. Kept apart from the runner, the environment, the files and the
// process spawner given, so it can be tested without any of them.
import { accessSync, constants, existsSync } from "node:fs";
import { delimiter, join } from "node:path";

/** The package that installs Xvfb, named when it is missing (Debian's and Ubuntu's; Fedora's is
 *  xorg-x11-server-Xvfb). */
export const XVFB_PACKAGE = "xvfb";
/** The virtual display's screen: as large as the app's biggest window, in full colour. */
export const XVFB_SCREEN = "1920x1080x24";
/** The first display number tried, then the next ones: above the ones sessions take (a desktop's
 *  :0, a VNC server's :1, …), as xvfb-run's. */
export const FIRST_DISPLAY = 99;
/** How many display numbers are tried, the taken ones passed over. */
const DISPLAYS_TRIED = 100;
/** How many times Xvfb is started on a free-looking number before giving up (another one took it
 *  between the look and the start). */
const STARTS = 3;

/** Whether a run on `platform` with `env` needs a display of its own: Linux with neither an X nor
 *  a Wayland one.
 *  @param {string} platform  process.platform
 *  @param {Record<string, string | undefined>} env
 *  @returns {boolean} */
export const needsDisplay = (platform, env) => platform === "linux" && !env.DISPLAY && !env.WAYLAND_DISPLAY;

/** Whether `command` is an executable in one of `path`'s folders.
 *  @param {string} command
 *  @param {string} [path]
 *  @returns {boolean} */
export function onPath(command, path = process.env.PATH ?? "") {
  return path
    .split(delimiter)
    .filter(Boolean)
    .some((dir) => {
      try {
        accessSync(join(dir, command), constants.X_OK);
        return true;
      } catch {
        return false;
      }
    });
}

/** An X server's files for display `n` (under `root`, the system's /tmp): its lock and its
 *  socket. Either there means the number is taken: a server whose socket went keeps its lock.
 *  @param {number} n
 *  @param {string} [root] */
export const displayFiles = (n, root = "/tmp") => ({ lock: `${root}/.X${n}-lock`, socket: `${root}/.X11-unix/X${n}` });

/**
 * The run's display: none to start when there is one already (or not on Linux); otherwise Xvfb on
 * the first number from FIRST_DISPLAY whose lock and socket are both absent (never `-displayfd`:
 * Xvfb takes a number with it without checking the number's lock), up when its socket is there.
 * `:<n>` returned with `stop` (idempotent, synchronous: safe in an `exit` handler); why not, when
 * Xvfb isn't installed, no number is free, or it exits or doesn't come up within `timeoutMs`.
 * @param {{
 *   platform?: string,
 *   env?: Record<string, string | undefined>,
 *   spawn: (command: string, args: string[], options: object) => import("node:child_process").ChildProcess,
 *   installed?: (command: string) => boolean,
 *   exists?: (path: string) => boolean,
 *   root?: string,
 *   output?: number | "ignore",
 *   log?: string,
 *   timeoutMs?: number,
 *   pollMs?: number,
 * }} opts  `output` the fd Xvfb's own output goes to (`log` its file, named in a failure)
 * @returns {Promise<{ display: null } | { display: string, stop: () => void } | { error: string }>}
 */
export async function startDisplay({
  platform = process.platform,
  env = process.env,
  spawn,
  installed = (c) => onPath(c, env.PATH),
  exists = existsSync,
  root = "/tmp",
  output = "ignore",
  log = "",
  timeoutMs = 15000,
  pollMs = 100,
}) {
  if (!needsDisplay(platform, env)) return { display: null };
  if (!installed("Xvfb"))
    return {
      error: `no display here (DISPLAY and WAYLAND_DISPLAY are empty) and no Xvfb to start one: install the ${XVFB_PACKAGE} package (apt install ${XVFB_PACKAGE}; xorg-x11-server-Xvfb on Fedora), or run in a desktop session`,
    };
  const see = log ? ` (its output: ${log})` : "";
  let why = `no free display number from :${FIRST_DISPLAY} to :${FIRST_DISPLAY + DISPLAYS_TRIED - 1}`;
  let starts = 0;
  for (let n = FIRST_DISPLAY; n < FIRST_DISPLAY + DISPLAYS_TRIED && starts < STARTS; n++) {
    const files = displayFiles(n, root);
    if (exists(files.lock) || exists(files.socket)) continue;
    starts += 1;
    const up = await startOn(n, { spawn, exists, root, output, timeoutMs, pollMs });
    if ("stop" in up) return up;
    why = `${up.error}${see}`;
    // Taken between the look and the start: the next number. Anything else won't go better there.
    if (!up.taken) break;
  }
  return { error: why };
}

/** Xvfb started on `:n` and waited for (its socket there); its `stop`, or why not (`taken` when
 *  another server holds the number now).
 *  @param {number} n
 *  @param {{ spawn: Function, exists: (path: string) => boolean, root: string, output: number | "ignore", timeoutMs: number, pollMs: number }} opts
 *  @returns {Promise<{ display: string, stop: () => void } | { error: string, taken: boolean }>} */
async function startOn(n, { spawn, exists, root, output, timeoutMs, pollMs }) {
  // Its own process group: a Ctrl-C at the terminal reaches the runner, which stops it after the
  // apps drawing on it, not before.
  /** @type {import("node:child_process").ChildProcess} */
  const proc = spawn("Xvfb", [`:${n}`, "-nolisten", "tcp", "-screen", "0", XVFB_SCREEN], { stdio: ["ignore", output, output], detached: true });
  const stop = () => {
    if (proc.exitCode === null && proc.signalCode === null && !proc.killed) {
      try {
        proc.kill("SIGTERM");
      } catch {
        /* gone already */
      }
    }
  };
  /** @type {string | null} */
  let failed = null;
  proc.on("error", (e) => (failed = `Xvfb couldn't be started: ${e.message}`));
  proc.on("exit", (code, signal) => (failed = `Xvfb exited (${signal ?? `code ${code}`}) before its display :${n} came up`));
  const { socket, lock } = displayFiles(n, root);
  for (let waited = 0; waited < timeoutMs; waited += pollMs) {
    await new Promise((r) => setTimeout(r, pollMs));
    if (failed) return { error: failed, taken: exists(lock) };
    if (exists(socket)) return { display: `:${n}`, stop };
  }
  stop();
  return { error: `Xvfb's display :${n} didn't come up within ${Math.round(timeoutMs / 1000)} s`, taken: false };
}
