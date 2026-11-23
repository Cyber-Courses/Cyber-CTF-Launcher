// What the runner checks before it starts the app, and what it adds when its first app doesn't
// come up (run.mjs `appComesUp`): the likely cause, after the app's own message, only as far as
// that message tells it. Kept apart from the runner so it can be tested without an app.
import { closeSync, openSync, readSync } from "node:fs";

/** What a `wdio` build's binary holds and no other build's (src-tauri/src/platform/test_root.rs
 *  `BUILD_MARK`, the same text). Only such a build keeps to the run's folder (CYBERCTF_WDIO_ROOT);
 *  another ignores it and uses the user's own: their labs, settings, session and runner service. */
export const WDIO_BUILD_MARK = "cyberctf-wdio-build: keeps an end-to-end run's folder (CYBERCTF_WDIO_ROOT)";

/** How to get a `wdio` build, for the refusal. */
export const WDIO_REBUILD =
  "rebuild it with --features wdio: tests/e2e/desktop/runner/run.sh (run.ps1) without --skip-build or --app builds and keeps its own copy, " +
  "out of reach of a `cargo test` or `cargo build` without the feature; a package must come from `NEXT_PUBLIC_WDIO=1 pnpm tauri build --features wdio`";

/** Whether `file` holds `mark`, read a chunk at a time (a debug build is hundreds of MB). */
export function holds(file, mark = WDIO_BUILD_MARK, chunk = 8 << 20) {
  const needle = Buffer.from(mark);
  const buf = Buffer.alloc(chunk + needle.length - 1);
  const fd = openSync(file, "r");
  try {
    let kept = 0;
    for (;;) {
      const read = readSync(fd, buf, kept, chunk, null);
      if (read === 0) return false;
      const end = kept + read;
      if (buf.subarray(0, end).includes(needle)) return true;
      // The mark may straddle two chunks: the last bytes are read again with the next.
      kept = Math.min(needle.length - 1, end);
      buf.copy(buf, 0, end - kept, end);
    }
  } finally {
    closeSync(fd);
  }
}

/** Why the runner won't start `app` (null when it is a `wdio` build): checked on the file, never
 *  by starting it, which is what a build without the feature must never be under a run.
 *  @param {string} app the app's binary (the one inside a wrapper: `binary`)
 *  @param {string} [binary] the file the app runs when `app` only wraps it (an AppImage's) */
export function notWdioBuild(app, binary = app) {
  let marked;
  try {
    marked = holds(binary);
  } catch (e) {
    return `couldn't read ${binary} to check it is a wdio build: ${e.message}`;
  }
  if (marked) return null;
  const which = binary === app ? app : `${app} (its binary ${binary})`;
  return `${which} isn't a wdio build (it doesn't hold "${WDIO_BUILD_MARK}"): it would ignore the run's folder and use your own data folder and runner service, so it isn't started; ${WDIO_REBUILD}`;
}

/** What GTK says, or the app's window toolkit (tao) panics with, when it finds no display. */
export const NO_DISPLAY = /Failed to initialize gtk|cannot open display|Gtk-WARNING \*\*: .*cannot open display/i;

/** What the WebDriver plugin logs when it starts: only a build with `--features wdio` has it. */
export const WDIO_STARTED = /WebDriver server listening|WDIO WebDriver plugin initialized/;

/** The hint after `why` (appComesUp's reason), or "" when `why` says it all (the port held, the
 *  port not bound). `output` is all the app printed (`log` its file), `why` only its last lines.
 *  Never a build without `--features wdio`: the runner refuses one before it starts it
 *  (`notWdioBuild`).
 *  - The app exited: no display when its output says so, else its own message's cause.
 *  - It ran and nothing answered: its WebDriver plugin said it started or not, and the causes left.
 *  @param {string} why
 *  @param {{ env?: Record<string, string | undefined>, log?: string, output?: string }} [opts]
 *  @returns {string} */
export function preflightHint(why, { env = process.env, log = "", output = "" } = {}) {
  const see = log ? ` (all of its output: ${log})` : "";
  if (/^the app exited \(/.test(why)) {
    if (NO_DISPLAY.test(why) || NO_DISPLAY.test(output))
      return `the app found no display to open its window on (DISPLAY=${env.DISPLAY || "(empty)"}, WAYLAND_DISPLAY=${env.WAYLAND_DISPLAY || "(empty)"}): on Linux the runner's own Xvfb (its output in xvfb.log beside the specs' logs) didn't take it`;
    return `its own message above says why it stopped${see}: a missing system library, no display, or a crash at start are the usual causes (it is a wdio build: the runner checked it before it started it)`;
  }
  if (/^no embedded WebDriver server on /.test(why)) {
    if (WDIO_STARTED.test(output))
      return `the app's WebDriver plugin says it started${see}, so the build has it: something on 127.0.0.1 keeps its port from answering, or the app is too slow to`;
    return `the app ran, but nothing answered on its WebDriver port${see}: it is a wdio build (the runner checked it before it started it), so it is slow to start, its WebDriver plugin failed, or something blocks the port on 127.0.0.1`;
  }
  return "";
}
