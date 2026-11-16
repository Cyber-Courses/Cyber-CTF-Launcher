import { describe, expect, it } from "vitest";
import { preflightHint } from "./preflight.mjs";

// What the runner adds when its first app doesn't come up (runner/preflight.mjs): the likely cause,
// only as far as the app's own message tells it.

const log = "/logs/preflight.log";
const headless = { DISPLAY: "", WAYLAND_DISPLAY: "" };
/** What tao panics with on a Linux without a display, as appComesUp words it. */
const gtkPanic =
  "the app exited (code 101) before its WebDriver server answered: thread 'main' panicked at /cargo/registry/tao-0.37.1/src/platform_impl/linux/event_loop.rs:217:53: | Failed to initialize gtk backend!: BoolError { message: \"Failed to initialize GTK\" } | note: run with `RUST_BACKTRACE=1`";

describe("the hint after an app that didn't come up", () => {
  it("names the display when the app says it found none, never the wdio build", () => {
    const hint = preflightHint(gtkPanic, { env: headless, log });
    expect(hint).toMatch(/^the app found no display to open its window on \(DISPLAY=\(empty\), WAYLAND_DISPLAY=\(empty\)\)/);
    expect(hint).not.toContain("--features wdio");
    expect(
      preflightHint("the app exited (code 1) before its WebDriver server answered: Gtk-WARNING **: 10:00: cannot open display: :7", { env: { DISPLAY: ":7" } }),
    ).toContain("DISPLAY=:7");
  });

  it("points at the app's own message for another exit, a build without wdio only as what it isn't", () => {
    const hint = preflightHint("the app exited (code 127) before its WebDriver server answered: error while loading shared libraries: libwebkit2gtk-4.1.so.0", {
      log,
    });
    expect(hint).toMatch(
      /^its own message above says why it stopped \(all of its output: \/logs\/preflight.log\): a missing system library, no display, or a crash/,
    );
    expect(hint).toContain("an app built without --features wdio doesn't exit");
  });

  it("names a build without wdio as the likely cause when the app ran and nothing answered, among others", () => {
    const hint = preflightHint("no embedded WebDriver server on 4445 within 60 s: started", { log });
    expect(hint).toMatch(/most likely it was built without --features wdio/);
    expect(hint).toMatch(/otherwise it is slow to start or something blocks the port/);
  });

  it("says the build has wdio when its WebDriver plugin said it started, though nothing answered", () => {
    const output = "[INFO][tauri_plugin_wdio_webdriver::server] WebDriver server listening on http://127.0.0.1:4445\n";
    const hint = preflightHint("no embedded WebDriver server on 4445 within 60 s: started", { log, output });
    expect(hint).toMatch(/^the app's WebDriver plugin says it started .*so the build has it/);
    expect(hint).not.toContain("built without");
  });

  it("reads the display's failure in all of the app's output, not only its last lines", () => {
    const output = "Gtk-WARNING **: cannot open display: \nmore\nlines\n";
    expect(preflightHint("the app exited (code 1) before its WebDriver server answered: more | lines", { env: headless, output })).toMatch(
      /^the app found no display/,
    );
  });

  it("adds nothing when the reason says it all: the port held or not bound", () => {
    expect(preflightHint("port 4445 is held by cyberctf-desktop (pid 12): the run would drive that app instead of its own")).toBe("");
    expect(preflightHint("the app couldn't bind its WebDriver port 4445: Failed to bind WebDriver server")).toBe("");
  });
});
