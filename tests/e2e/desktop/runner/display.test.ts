import type { ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  FIRST_DISPLAY,
  SESSION_BUS,
  SESSION_BUS_PACKAGE,
  WINDOW_MANAGERS,
  XVFB_PACKAGE,
  busSocket,
  displayFiles,
  needsDisplay,
  onPath,
  runEnv,
  startDisplay,
} from "./display.mjs";

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
    public options: { stdio: unknown[]; detached: boolean; env?: Record<string, string | undefined> },
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
 *  socket made), fake session buses (in `buses`), doing `busThen` with each (by default: its
 *  socket made), and fake window managers (in `wms`), doing `wmThen` with each (by default:
 *  nothing, it keeps running). */
function machine(
  files: string[] = [],
  then?: (x: FakeXvfb, n: number, fs: Set<string>) => void,
  wmThen?: (wm: FakeXvfb) => void,
  busThen?: (bus: FakeXvfb, socket: string, fs: Set<string>) => void,
) {
  const fs = new Set(files);
  const started: FakeXvfb[] = [];
  const buses: FakeXvfb[] = [];
  const wms: FakeXvfb[] = [];
  const comesUp = (_x: FakeXvfb, n: number) => {
    const files = displayFiles(n);
    fs.add(files.lock);
    fs.add(files.socket);
  };
  const spawn = (command: string, args: string[], options: object) => {
    const x = new FakeXvfb(command, args, options as FakeXvfb["options"]);
    if (command === SESSION_BUS) {
      buses.push(x);
      const socket = (args.find((a) => a.startsWith("--address=unix:path=")) ?? "").slice("--address=unix:path=".length);
      setTimeout(() => {
        (busThen ?? ((_b, at, files) => files.add(at)))(x, socket, fs);
      }, 1);
      return x as unknown as ChildProcess;
    }
    if (command !== "Xvfb") {
      wms.push(x);
      if (wmThen)
        setTimeout(() => {
          wmThen(x);
        }, 1);
      return x as unknown as ChildProcess;
    }
    started.push(x);
    const n = Number(args[0].slice(1));
    setTimeout(() => {
      (then ?? comesUp)(x, n, fs);
    }, 1);
    return x as unknown as ChildProcess;
  };
  /** What startDisplay is given: the fake spawner and files, Xvfb and every window manager
   *  installed, a short poll and settle. */
  const opts = { spawn, exists: (p: string) => fs.has(p), installed: () => true, pollMs: 2, settleMs: 10, pid: 4242 };
  return { opts, started, buses, wms };
}

const headless = { platform: "linux", env: { PATH: "/usr/bin" } };

describe("whether the run gets a display of its own", () => {
  it("does on Linux, always; not on macOS or Windows", () => {
    expect(needsDisplay("linux")).toBe(true);
    expect(needsDisplay("darwin")).toBe(false);
    expect(needsDisplay("win32")).toBe(false);
  });
});

/** A desktop's or a VNC session's shell: its display, Wayland's, its session bus and its
 *  accessibility bus. */
const session = {
  PATH: "/usr/bin",
  DISPLAY: ":1",
  WAYLAND_DISPLAY: "wayland-0",
  DBUS_SESSION_BUS_ADDRESS: "unix:path=/run/user/1000/bus",
  AT_SPI_BUS_ADDRESS: "unix:path=/run/user/1000/at-spi/bus",
};

describe("the environment on the run's display", () => {
  it("has the run's display and bus, GTK on X, and nothing of the session's", () => {
    const env = runEnv(session, ":99", "unix:path=/tmp/b");
    expect(env).toEqual({ PATH: "/usr/bin", DISPLAY: ":99", DBUS_SESSION_BUS_ADDRESS: "unix:path=/tmp/b", GDK_BACKEND: "x11" });
    expect(session.WAYLAND_DISPLAY).toBe("wayland-0");
  });
});

describe("the run's display", () => {
  it("is none to start off Linux", async () => {
    const m = machine();
    expect(await startDisplay({ ...m.opts, platform: "darwin", env: {} })).toEqual({ display: null });
    expect(await startDisplay({ ...m.opts, platform: "win32", env: session })).toEqual({ display: null });
    expect(m.started).toEqual([]);
  });

  it("is the run's own on Linux from a desktop or a VNC session too: their display, Wayland's and their bus reach none of what it starts", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, platform: "linux", env: session });
    const socket = busSocket(FIRST_DISPLAY, 4242);
    expect(d).toMatchObject({ display: ":99", bus: { address: `unix:path=${socket}` }, windowManager: { name: "xfwm4" } });
    expect(m.started.map((x) => x.command)).toEqual(["Xvfb"]);
    for (const p of [...m.buses, ...m.wms]) {
      expect(p.options.env).toMatchObject({ DISPLAY: ":99", DBUS_SESSION_BUS_ADDRESS: `unix:path=${socket}` });
      expect(p.options.env).not.toHaveProperty("WAYLAND_DISPLAY");
      expect(p.options.env).not.toHaveProperty("AT_SPI_BUS_ADDRESS");
    }
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

  it("is stopped by its `stop`, its window manager with it, once however often it is called", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless });
    if (!("stop" in d)) throw new Error("no display started");
    d.stop();
    d.stop();
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
    expect(m.wms[0].kills).toEqual(["SIGTERM"]);
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

describe("the window manager on the runner's Xvfb", () => {
  /** startDisplay with only `have` of the window managers installed (Xvfb and the bus always). */
  const only = (have: string[]) => (c: string) => c === "Xvfb" || c === SESSION_BUS || have.includes(c);

  it("is the first installed of xfwm4, openbox and fluxbox, started on Xvfb's display after it is up", async () => {
    expect(WINDOW_MANAGERS.map((w) => w.command)).toEqual(["xfwm4", "openbox", "fluxbox"]);
    for (const [have, want] of [
      [["xfwm4", "openbox", "fluxbox"], "xfwm4"],
      [["openbox", "fluxbox"], "openbox"],
      [["fluxbox"], "fluxbox"],
    ] as const) {
      const m = machine();
      const d = await startDisplay({ ...m.opts, ...headless, installed: only([...have]) });
      expect(d).toMatchObject({ display: ":99", windowManager: { name: want } });
      expect(m.wms.map((w) => w.command)).toEqual([want]);
      const [wm] = m.wms;
      expect(wm.options.env).toMatchObject({ DISPLAY: ":99", PATH: "/usr/bin" });
      expect(wm.options.detached).toBe(true);
    }
  });

  it("runs without a session manager or a compositor, which a bare Xvfb has neither of", async () => {
    const m = machine();
    await startDisplay({ ...m.opts, ...headless });
    expect(m.wms[0].args).toEqual(["--compositor=off", "--sm-client-disable"]);
  });

  it("isn't there when none is installed: the display is, with why", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless, installed: only([]) });
    expect(d).toMatchObject({ display: ":99", windowManager: { why: "none installed (xfwm4, openbox, fluxbox)" } });
    expect(m.wms).toEqual([]);
    if (!("stop" in d)) throw new Error("no display started");
    d.stop();
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
  });

  it("isn't there when it ends as it starts: the display is, with why and its log named", async () => {
    const m = machine([], undefined, (wm) => {
      wm.exits(1);
    });
    const d = await startDisplay({ ...m.opts, ...headless, log: "/logs/xvfb.log" });
    expect(d).toMatchObject({ display: ":99", windowManager: { why: "xfwm4 exited (code 1) as it started on :99 (its output: /logs/xvfb.log)" } });
    if (!("stop" in d)) throw new Error("no display started");
    d.stop();
    expect(m.wms[0].kills).toEqual([]);
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
  });

  it("isn't there when it can't be spawned", async () => {
    const m = machine([], undefined, (wm) => wm.emit("error", new Error("spawn xfwm4 EACCES")));
    const d = await startDisplay({ ...m.opts, ...headless });
    expect(d).toMatchObject({ windowManager: { why: "xfwm4 couldn't be started: spawn xfwm4 EACCES" } });
  });

  it("isn't started when Xvfb's display isn't up", async () => {
    const m = machine([], (x) => {
      x.exits(1);
    });
    await startDisplay({ ...m.opts, ...headless });
    expect(m.wms).toEqual([]);
  });
});

describe("the session bus beside the runner's Xvfb", () => {
  // The desktop session's bus belongs to another display: a desktop service there that never
  // answers (a broken xdg-desktop-portal) held every app start for D-Bus's 25 s timeouts, and
  // the specs' apps never loaded their page in time.
  const socket = busSocket(FIRST_DISPLAY, 4242);

  it("is a bus of the run's own, on a socket of the display's, its address given once the socket is there", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless, env: { PATH: "/usr/bin", DBUS_SESSION_BUS_ADDRESS: "unix:path=/run/user/1000/bus" } });
    expect(d).toMatchObject({ display: ":99", bus: { address: `unix:path=${socket}` } });
    const [bus] = m.buses;
    expect(bus.command).toBe("dbus-daemon");
    expect(bus.args).toEqual(["--session", "--nofork", "--nopidfile", `--address=unix:path=${socket}`]);
    expect(bus.options.detached).toBe(true);
    // What it starts on demand (the portal, the accessibility bus) draws on the run's display.
    expect(bus.options.env).toMatchObject({ DISPLAY: ":99" });
  });

  it("is never another run's: the socket names the display and the runner", () => {
    expect(busSocket(99, 4242)).toBe("/tmp/.cyberctf-e2e-bus-99-4242");
    expect(busSocket(100, 7, "/x")).toBe("/x/.cyberctf-e2e-bus-100-7");
  });

  it("is the window manager's bus too", async () => {
    const m = machine();
    await startDisplay({ ...m.opts, ...headless, env: { PATH: "/usr/bin", DBUS_SESSION_BUS_ADDRESS: "unix:path=/run/user/1000/bus" } });
    expect(m.wms[0].options.env).toMatchObject({ DISPLAY: ":99", DBUS_SESSION_BUS_ADDRESS: `unix:path=${socket}` });
  });

  it("is stopped by the display's `stop`, once, with the window manager and Xvfb", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless });
    if (!("stop" in d)) throw new Error("no display started");
    d.stop();
    d.stop();
    expect(m.buses[0].kills).toEqual(["SIGTERM"]);
    expect(m.wms[0].kills).toEqual(["SIGTERM"]);
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
  });

  it("is needed: without dbus-daemon installed nothing starts, and why names the package", async () => {
    const m = machine();
    const d = await startDisplay({ ...m.opts, ...headless, installed: (c: string) => c !== SESSION_BUS });
    expect(d).toEqual({ error: expect.stringContaining(`install the ${SESSION_BUS_PACKAGE} package`) as unknown });
    expect(m.started).toEqual([]);
  });

  it("is needed: when it ends as it starts, why with its log named, and the display stopped", async () => {
    const m = machine([], undefined, undefined, (bus) => {
      bus.exits(1);
    });
    const d = await startDisplay({ ...m.opts, ...headless, log: "/logs/xvfb.log" });
    expect(d).toEqual({ error: "dbus-daemon exited (code 1) as it started (its output: /logs/xvfb.log)" });
    expect(m.buses[0].kills).toEqual([]);
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
    expect(m.wms).toEqual([]);
  });

  it("is needed: when its socket never comes, it and the display stopped, with why", async () => {
    const m = machine([], undefined, undefined, () => {});
    const d = await startDisplay({ ...m.opts, ...headless, timeoutMs: 20 });
    expect(d).toEqual({ error: expect.stringMatching(/^dbus-daemon's socket didn't come within/) as unknown });
    expect(m.buses[0].kills).toEqual(["SIGTERM"]);
    expect(m.started[0].kills).toEqual(["SIGTERM"]);
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
