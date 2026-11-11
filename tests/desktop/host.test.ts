import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { dockerPluginDirs, portFree, portHolder, portStillHeld, waitPortFree } from "./lib/host.mjs";
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
