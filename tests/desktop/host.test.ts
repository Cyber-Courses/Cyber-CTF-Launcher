import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { dockerPluginDirs, engineSilence, portFree, portHolder, portStillHeld, runBounded, utmUnreachable, waitPortFree } from "./lib/host.mjs";
import { deadDockerEnv } from "./lib/support.mjs";

// What the desktop end-to-end reads of the machine (lib/host.mjs): the app's WebDriver port, and
// where the Docker CLI finds Compose, which the dead-Docker knob (support.mjs) must keep.

const dirs: string[] = [];
const servers: Server[] = [];
afterEach(() => {
  for (const s of servers.splice(0)) s.close();
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

const tempDir = (prefix: string) => {
  const d = mkdtempSync(join(tmpdir(), prefix));
  dirs.push(d);
  return d;
};

/** A server holding a free port on 127.0.0.1, and that port. */
async function holding(): Promise<number> {
  const s = createServer();
  servers.push(s);
  await new Promise<void>((done) => s.listen(0, "127.0.0.1", done));
  const address = s.address();
  if (address === null || typeof address === "string") throw new Error("no port");
  return address.port;
}

describe("the WebDriver port", () => {
  it("is free when nothing listens on it, held while something does, and said who holds it", async () => {
    const port = await holding();
    expect(await portFree(port)).toBe(false);
    expect(await waitPortFree(port, 0.3)).toBe(false);
    if (process.platform !== "win32" && spawnSync("lsof", ["-v"]).error === undefined)
      expect(portHolder(port)).toMatch(new RegExp(`^pid ${String(process.pid)}(: .+)?$`));
    expect(portStillHeld(port, "pid 7: app")).toBe(`the previous app still holds port ${String(port)} (pid 7: app)`);
    expect(portStillHeld(port, "")).toBe(`the previous app still holds port ${String(port)}`);
    const s = servers.pop();
    await new Promise((done) => s?.close(done));
    expect(await waitPortFree(port, 2)).toBe(true);
  });
});

describe("UTM from this session", () => {
  it("can't be driven from an SSH session on a Mac: the VM specs on it skip, saying why", () => {
    expect(utmUnreachable("darwin", "Background")).toBe(
      "can't run here over SSH: no Apple events to UTM (-600): this session isn't the Mac's GUI one (launchctl: Background)",
    );
  });

  it("is driven from the GUI session, and isn't asked about elsewhere", () => {
    expect(utmUnreachable("darwin", "Aqua")).toBeNull();
    expect(utmUnreachable("darwin", "")).toBeNull();
    expect(utmUnreachable("linux")).toBeNull();
    expect(utmUnreachable("win32")).toBeNull();
  });
});

describe("the Docker CLI's plugin folders", () => {
  it("are its config folder's and the ones its config lists, those that are there", () => {
    const config = tempDir("docker-config-");
    const extra = tempDir("docker-plugins-");
    mkdirSync(join(config, "cli-plugins"));
    writeFileSync(join(config, "config.json"), JSON.stringify({ currentContext: "desktop-linux", cliPluginsExtraDirs: [extra, join(extra, "gone"), 3] }));
    expect(dockerPluginDirs({ DOCKER_CONFIG: config }, "/nowhere")).toEqual([join(config, "cli-plugins"), extra]);
    // No DOCKER_CONFIG: the home's ~/.docker.
    const home = tempDir("docker-home-");
    mkdirSync(join(home, ".docker", "cli-plugins"), { recursive: true });
    expect(dockerPluginDirs({}, home)).toEqual([join(home, ".docker", "cli-plugins")]);
    expect(dockerPluginDirs({}, tempDir("docker-none-"))).toEqual([]);
  });

  it("are kept by the dead-Docker knob, which names no context", () => {
    const root = tempDir("cyberctf-e2e-host-");
    const before = process.env.CYBERCTF_WDIO_ROOT;
    process.env.CYBERCTF_WDIO_ROOT = root;
    try {
      const env = deadDockerEnv();
      const config = JSON.parse(readFileSync(join(env.DOCKER_CONFIG, "config.json"), "utf8")) as Record<string, unknown>;
      expect(config.currentContext).toBeUndefined();
      expect(config.cliPluginsExtraDirs ?? []).toEqual(dockerPluginDirs());
      // Where Compose is installed here, the knob stops the engine and leaves Compose: a lab's
      // `docker compose` says it can't reach the engine, never that it doesn't know the command.
      const composeHere = spawnSync("docker", ["compose", "version"], { encoding: "utf8" }).status === 0;
      if (composeHere) {
        const ps = spawnSync("docker", ["compose", "--project-name", "rig-repro-knob", "ps", "--all", "--format", "json"], {
          encoding: "utf8",
          cwd: root,
          env: { ...process.env, ...env },
        });
        expect(ps.status).not.toBe(0);
        expect(ps.stderr).not.toMatch(/unknown (flag|command)/);
      }
    } finally {
      if (before === undefined) delete process.env.CYBERCTF_WDIO_ROOT;
      else process.env.CYBERCTF_WDIO_ROOT = before;
    }
  });
});

describe("a program run with a bound", () => {
  /** A Node script in a folder of its own, run as the program. */
  const script = (body: string) => {
    const file = join(tempDir("bounded-"), "program.mjs");
    writeFileSync(file, body);
    return file;
  };
  const alive = (pid: number) => {
    try {
      process.kill(pid, 0);
      return true;
    } catch {
      return false;
    }
  };

  it("answers its output and exit code, its stdin the null device", () => {
    const program = script(`let input = "";
process.stdin.on("data", (d) => (input += d));
process.stdin.on("end", () => { console.log("29.0.0 " + JSON.stringify(input)); process.exit(3); });`);
    expect(runBounded(process.execPath, [program], 10000)).toEqual({ out: '29.0.0 ""\n', code: 3, timedOut: false, missing: false });
  });

  it("ends a program past its bound with what it started, which held its output open", async () => {
    // Docker Desktop still starting: the CLI waits ~47 s, and a plugin it runs (Compose) outlives
    // a docker.exe ended alone, its output still open.
    const pids = join(tempDir("bounded-pids-"), "helper");
    const program = script(`import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
const helper = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: ["ignore", "inherit", "ignore"] });
writeFileSync(${JSON.stringify(pids)}, String(helper.pid));
console.log("partial");
setInterval(() => {}, 1000);`);
    const start = Date.now();
    const r = runBounded(process.execPath, [program], 1500);
    expect(Date.now() - start).toBeLessThan(10000);
    expect(r).toEqual({ out: "partial\n", code: null, timedOut: true, missing: false });
    const helper = Number(readFileSync(pids, "utf8"));
    await expect.poll(() => alive(helper), { timeout: 5000 }).toBe(false);
  });

  it("says when the program isn't installed", () => {
    expect(runBounded("cyberctf-no-such-program", [], 5000)).toEqual({ out: "", code: null, timedOut: false, missing: true });
  });
});

describe("whether docker info says an engine answers", () => {
  const info = (p: Partial<{ out: string; code: number | null; timedOut: boolean; missing: boolean }>) => ({
    out: "",
    code: 0,
    timedOut: false,
    missing: false,
    ...p,
  });

  it("is its version named, never an exit 0 alone: Docker 26's answer for a daemon that doesn't answer", () => {
    expect(engineSilence(info({ out: "29.0.0\n" }), 20)).toBeNull();
    // Debian 13's docker.io 26.1.5 with a dead DOCKER_HOST (jarvis, debian13-quick e2e.shell, #291).
    expect(engineSilence(info({ out: "\n" }), 20)).toBe("an empty answer: its daemon doesn't answer");
    expect(engineSilence(info({ code: null, timedOut: true }), 20)).toBe("no answer in 20 s");
    expect(engineSilence(info({ code: null, missing: true }), 20)).toBe("not installed");
    expect(engineSilence(info({ code: 1 }), 20)).toBe("exit 1");
  });
});
