import type { ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { FIRST_DISPLAY, XVFB_PACKAGE, displayFiles, needsDisplay, onPath, startDisplay } from "./display.mjs";

// The run's display (runner/display.mjs): when the runner starts Xvfb, on which display, and that
// it stops it, given a fake environment, fake X server files and a fake process spawner. No Xvfb is started.

/** A fake Xvfb: what it was started with, and its kills. */
class FakeXvfb extends EventEmitter {
  exitCode: number | null = null;
  signalCode: string | null = null;
  killed = false;
  kills: string[] = [];
  constructor(
    public command: string,
    public args: string[],
    public options: { stdio: unknown[]; detached: boolean },
  ) {
    super();
  }
  kill(signal: string) {
    this.kills.push(signal);
    this.killed = true;
    queueMicrotask(() => this.emit("exit", null, signal));
    return true;
  }
  /** Xvfb exiting by itself, as when it can't start. */
  exits(code: number) {
    this.exitCode = code;
    this.emit("exit", code, null);
  }
}

/** A fake machine: the X servers' files there (`files`), and a spawner that hands out FakeXvfbs,
 *  doing `then` with each once it is started (by default: its display comes up, its lock and
 *  socket made). */
function machine(files: string[] = [], then?: (x: FakeXvfb, n: number, fs: Set<string>) => void) {
  const fs = new Set(files);
  const started: FakeXvfb[] = [];
  const comesUp = (_x: FakeXvfb, n: number) => {
    const files = displayFiles(n);
    fs.add(files.lock);
    fs.add(files.socket);
  };
  const spawn = (command: string, args: string[], options: object) => {
    const x = new FakeXvfb(command, args, options as FakeXvfb["options"]);
    started.push(x);
    const n = Number(args[0].slice(1));
    setTimeout(() => {
      (then ?? comesUp)(x, n, fs);
    }, 1);
    return x as unknown as ChildProcess;
  };
  /** What startDisplay is given: the fake spawner and files, Xvfb installed, a short poll. */
  const opts = { spawn, exists: (p: string) => fs.has(p), installed: () => true, pollMs: 2 };
  return { opts, started };
}

const headless = { platform: "linux", env: { PATH: "/usr/bin" } };

describe("whether the run needs a display of its own", () => {
  it("does on Linux with neither DISPLAY nor WAYLAND_DISPLAY (empty counts as unset)", () => {
    expect(needsDisplay("linux", {})).toBe(true);
    expect(needsDisplay("linux", { DISPLAY: "", WAYLAND_DISPLAY: "" })).toBe(true);
  });

  it("doesn't with an X or a Wayland display, nor on macOS or Windows", () => {
    expect(needsDisplay("linux", { DISPLAY: ":0" })).toBe(false);
    expect(needsDisplay("linux", { WAYLAND_DISPLAY: "wayland-0" })).toBe(false);
    expect(needsDisplay("darwin", {})).toBe(false);
    expect(needsDisplay("win32", {})).toBe(false);
  });
});

describe("the run's display", () => {
  it("is the one there already: nothing started", async () => {
    const m = machine();
    expect(await startDisplay({ ...m.opts, platform: "linux", env: { DISPLAY: ":0" } })).toEqual({ display: null });
    expect(await startDisplay({ ...m.opts, platform: "darwin", env: {} })).toEqual({ display: null });
    expect(m.started).toEqual([]);
  });

  it("is Xvfb's on a headless Linux, on the first number from :99 no server holds, up once its socket is there", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless });
    expect(d).toMatchObject({ display: `:${String(FIRST_DISPLAY)}` });
    const [x] = m.started;
    expect(x.command).toBe("Xvfb");
    expect(x.args[0]).toBe(`:${String(FIRST_DISPLAY)}`);
    // -displayfd takes a number without checking its lock: never used.
    expect(x.args).not.toContain("-displayfd");
    expect(x.args).toEqual(expect.arrayContaining(["-nolisten", "tcp"]));
    expect(x.options.detached).toBe(true);
    expect(x.kills).toEqual([]);
  });

  it("passes over a number whose lock or socket is there: a server without its socket still holds it", async () => {
    const m = machine([displayFiles(99).lock, displayFiles(100).socket]);
    expect(await startDisplay({ ...m.opts, ...headless })).toMatchObject({ display: ":101" });
    expect(m.started.map((x) => x.args[0])).toEqual([":101"]);
  });

  it("takes the next number when another server took this one between the look and the start", async () => {
    const m = machine([], (x, n, fs) => {
      if (n === 99) {
        fs.add(displayFiles(99).lock);
        x.exits(1);
      } else {
        fs.add(displayFiles(n).socket);
      }
    });
    expect(await startDisplay({ ...m.opts, ...headless })).toMatchObject({ display: ":100" });
  });

  it("is stopped by its `stop`, once however often it is called", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless });
    if (!("stop" in d)) throw new Error("no display started");
    d.stop();
    d.stop();
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
  });

  it("isn't started when Xvfb isn't installed: why, naming the package", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless, installed: () => false });
    expect(d).toEqual({ error: expect.stringContaining(`install the ${XVFB_PACKAGE} package`) as unknown });
    expect(m.started).toEqual([]);
  });

  it("says why when Xvfb exits before its display comes up, its log named, and tries no further", async () => {
    const m = machine([], (x) => {
      x.exits(1);
    });
    const d = await startDisplay({ ...m.opts, ...headless, log: "/logs/xvfb.log" });
    expect(d).toEqual({ error: "Xvfb exited (code 1) before its display :99 came up (its output: /logs/xvfb.log)" });
    expect(m.started.length).toBe(1);
  });

  it("gives up on an Xvfb whose display doesn't come up in time, and stops it", async () => {
    const m = machine([], () => {});
    const d = await startDisplay({ ...m.opts, ...headless, timeoutMs: 20 });
    expect(d).toEqual({ error: expect.stringMatching(/^Xvfb's display :99 didn't come up within/) as unknown });
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
  });

  it("says why when Xvfb can't be spawned at all", async () => {
    const m = machine([], (x) => x.emit("error", new Error("spawn Xvfb EACCES")));
    expect(await startDisplay({ ...m.opts, ...headless })).toEqual({ error: "Xvfb couldn't be started: spawn Xvfb EACCES" });
  });

  it("says so when every number tried is taken", async () => {
    const taken = Array.from({ length: 100 }, (_, i) => displayFiles(FIRST_DISPLAY + i).lock);
    const m = machine(taken);
    expect(await startDisplay({ ...m.opts, ...headless })).toEqual({ error: "no free display number from :99 to :198" });
    expect(m.started).toEqual([]);
  });
});

describe("a command on the PATH", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
  });

  it.runIf(process.platform !== "win32")("is an executable in one of its folders", () => {
    const dir = mkdtempSync(join(tmpdir(), "display-path-"));
    dirs.push(dir);
    writeFileSync(join(dir, "Xvfb"), "#!/bin/sh\n");
    writeFileSync(join(dir, "plain"), "");
    chmodSync(join(dir, "Xvfb"), 0o755);
    expect(onPath("Xvfb", `/nowhere:${dir}`)).toBe(true);
    expect(onPath("plain", dir)).toBe(false);
    expect(onPath("Xvfb", "")).toBe(false);
  });
});
