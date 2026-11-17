// What the runner adds when its first app doesn't come up (run.mjs `appComesUp`): the likely
// cause, after the app's own message, only as far as that message tells it. Kept apart from the
// runner so it can be tested without an app.

/** What GTK says, or the app's window toolkit (tao) panics with, when it finds no display. */
export const NO_DISPLAY = /Failed to initialize gtk|cannot open display|Gtk-WARNING \*\*: .*cannot open display/i;

/** What the WebDriver plugin logs when it starts: only a build with `--features wdio` has it. */
export const WDIO_STARTED = /WebDriver server listening|WDIO WebDriver plugin initialized/;

/** The hint after `why` (appComesUp's reason), or "" when `why` says it all (the port held, the
 *  port not bound). `output` is all the app printed (`log` its file), `why` only its last lines.
 *  - The app exited: a build without `--features wdio` doesn't (it runs without the WebDriver
 *    server), so not that: no display when its output says so, else its own message's cause.
 *  - It ran and nothing answered: not such a build when its WebDriver plugin said it started,
 *    else most likely one, among other causes.
 *  @param {string} why
 *  @param {{ env?: Record<string, string | undefined>, log?: string, output?: string }} [opts]
 *  @returns {string} */
export function preflightHint(why, { env = process.env, log = "", output = "" } = {}) {
  const see = log ? ` (all of its output: ${log})` : "";
  if (/^the app exited \(/.test(why)) {
    if (NO_DISPLAY.test(why) || NO_DISPLAY.test(output))
      return `the app found no display to open its window on (DISPLAY=${env.DISPLAY || "(empty)"}, WAYLAND_DISPLAY=${env.WAYLAND_DISPLAY || "(empty)"}): on Linux the runner's own Xvfb (its output in xvfb.log beside the specs' logs) didn't take it`;
    return `its own message above says why it stopped${see}: a missing system library, no display, or a crash at start are the usual causes (an app built without --features wdio doesn't exit: it runs without the WebDriver server)`;
  }
  if (/^no embedded WebDriver server on /.test(why)) {
    if (WDIO_STARTED.test(output))
      return `the app's WebDriver plugin says it started${see}, so the build has it: something on 127.0.0.1 keeps its port from answering, or the app is too slow to`;
    return `the app ran, but nothing answered on its WebDriver port${see}: most likely it was built without --features wdio (a release build, or a cargo build without the feature); otherwise it is slow to start or something blocks the port on 127.0.0.1`;
  }
  return "";
}
