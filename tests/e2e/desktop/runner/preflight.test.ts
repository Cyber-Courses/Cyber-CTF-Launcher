import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { WDIO_BUILD_MARK, holds, notWdioBuild, preflightHint } from "./preflight.mjs";

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});
/** A file of `bytes`, in a folder removed after the test. */
function file(bytes: Buffer | string): string {
  const dir = mkdtempSync(join(tmpdir(), "desktop-preflight-"));
  dirs.push(dir);
  writeFileSync(join(dir, "cyberctf-desktop"), bytes);
  return join(dir, "cyberctf-desktop");
}

// What the runner checks before it ever starts the app: that it is a wdio build, the only one
// that keeps to the run's folder (runner/preflight.mjs `notWdioBuild`), read on its file.

describe("the check that the app is a wdio build", () => {
  it("looks for the mark the app's wdio build holds, the same text as the Rust side's", () => {
    const rust = readFileSync(join(import.meta.dirname, "..", "..", "..", "..", "src-tauri", "src", "platform", "test_root.rs"), "utf8");
    expect(rust).toContain(`#[cfg(feature = "wdio")]\npub static BUILD_MARK: &str = ${JSON.stringify(WDIO_BUILD_MARK)};`);
  });

  it("finds the mark anywhere in the file, across the chunks it reads", () => {
    const mark = Buffer.from(WDIO_BUILD_MARK);
    for (const at of [0, 1, 60, 64 - 10, 64, 200 - mark.length]) {
      const bytes = Buffer.alloc(200, 0x90);
      mark.copy(bytes, at);
      expect(holds(file(bytes), WDIO_BUILD_MARK, 64), `at ${String(at)}`).toBe(true);
    }
    expect(holds(file(Buffer.alloc(200, 0x90)), WDIO_BUILD_MARK, 64)).toBe(false);
    // Part of it is not it: a build that names the variable but isn't a wdio build.
    expect(holds(file(`${WDIO_BUILD_MARK.slice(0, -1)} CYBERCTF_WDIO_ROOT`), WDIO_BUILD_MARK, 16)).toBe(false);
    expect(holds(file(""), WDIO_BUILD_MARK, 16)).toBe(false);
  });

  it("passes a wdio build, and refuses another saying why and how to rebuild", () => {
    expect(notWdioBuild(file(`\x7fELF...${WDIO_BUILD_MARK}...`))).toBeNull();
    const plain = file("\x7fELF... a build without the feature ...");
    expect(notWdioBuild(plain)).toBe(
      `${plain} isn't a wdio build (it doesn't hold "${WDIO_BUILD_MARK}"): it would ignore the run's folder and use your own data folder and runner service, so it isn't started; ` +
        "rebuild it with --features wdio: tests/e2e/desktop/runner/run.sh (run.ps1) without --skip-build or --app builds and keeps its own copy, " +
        "out of reach of a `cargo test` or `cargo build` without the feature; a package must come from `NEXT_PUBLIC_WDIO=1 pnpm tauri build --features wdio`",
    );
    // A wrapper (an AppImage) is checked by the binary it runs, both named.
    expect(notWdioBuild("/pkg/Cyber.CTF.AppImage", plain)).toMatch(/^\/pkg\/Cyber\.CTF\.AppImage \(its binary .*cyberctf-desktop\) isn't a wdio build/);
    expect(notWdioBuild("/no/such/app")).toMatch(/^couldn't read \/no\/such\/app to check it is a wdio build: ENOENT/);
  });
});

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

  it("points at the app's own message for another exit, never a build without wdio (the runner checked)", () => {
    const hint = preflightHint("the app exited (code 127) before its WebDriver server answered: error while loading shared libraries: libwebkit2gtk-4.1.so.0", {
      log,
    });
    expect(hint).toMatch(
      /^its own message above says why it stopped \(all of its output: \/logs\/preflight.log\): a missing system library, no display, or a crash/,
    );
    expect(hint).toContain("it is a wdio build: the runner checked it before it started it");
  });

  it("names the causes left when the app ran and nothing answered, never a build without wdio", () => {
    const hint = preflightHint("no embedded WebDriver server on 4445 within 60 s: started", { log });
    expect(hint).toMatch(/it is a wdio build \(the runner checked it before it started it\), so it is slow to start/);
    expect(hint).toMatch(/or something blocks the port on 127\.0\.0\.1$/);
    expect(hint).not.toContain("built without");
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
