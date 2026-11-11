import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { PARSERS, parseProbe, readSnapshot } from "./inventory.ts";
import { KINDS } from "./kinds.ts";

// The leftover audit's kinds, held together: its listing (kinds.ts), its parsers, the kinds the
// three snapshot scripts record and the contract (tests/leftovers.md); then snapshot.sh run for
// real, with a Docker of its own, on a home and temp folder of its own.

const HERE = import.meta.dirname;
const read = (file: string) => readFileSync(join(HERE, file), "utf8");
/** The kinds a script records: the first argument of its probe calls (or emit/Add-Probe). */
const recorded = (source: string) =>
  new Set(
    [...source.matchAll(/(?:probe|rooted|emit|Invoke-Probe|Invoke-JsonProbe|Add-Probe)\s+["']?([a-z][a-z-]+[a-z])["']?\s/g)]
      .map((m) => m[1])
      .filter((k) => k in PARSERS),
  );

describe("the kinds", () => {
  const listed = KINDS.map((k) => k.kind);

  it("are listed once each, each with a parser, and every parser's kind is listed", () => {
    expect(new Set(listed).size).toBe(listed.length);
    expect([...listed].sort()).toEqual(Object.keys(PARSERS).sort());
  });

  it("are each recorded by the snapshot of every system the listing says reads it", () => {
    const scripts = { unix: recorded(read("snapshot.sh")), windows: recorded(read("snapshot.ps1")), server: recorded(read("snapshot-server.sh")) };
    for (const k of KINDS) {
      if (k.on.some((o) => o === "linux" || o === "macos")) expect(scripts.unix, k.kind).toContain(k.kind);
      if (k.on.includes("windows")) expect(scripts.windows, k.kind).toContain(k.kind);
      if (k.on.some((o) => o === "proxmox" || o === "esxi")) expect(scripts.server, k.kind).toContain(k.kind);
    }
    // ...and a script records no kind the listing doesn't have.
    for (const set of Object.values(scripts)) for (const kind of set) expect(listed).toContain(kind);
  });

  it("are each in the contract, tests/leftovers.md, as a row of its inventory", () => {
    const contract = readFileSync(join(HERE, "..", "leftovers.md"), "utf8");
    for (const kind of listed) expect(contract, kind).toMatch(new RegExp(`^\\| \`${kind}\` +\\|`, "m"));
  });

  it("are each allowed only with a reason, and only for a kind that exists", () => {
    for (const line of read("allowlist.tsv").split("\n")) {
      if (line === "" || line.startsWith("#")) continue;
      const [kind, , , reason] = line.split("\t");
      expect(listed).toContain(kind);
      expect(reason.length).toBeGreaterThan(20);
    }
  });
});

describe("snapshot.sh, run for real", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
  });

  /** snapshot.sh with `docker` answering `script` on PATH, HOME and TMPDIR its own (`only`: its
   *  --only groups). */
  function snapshot(docker: string, only = "") {
    const dir = mkdtempSync(join(tmpdir(), "audit-snapshot-test-"));
    dirs.push(dir);
    const bin = join(dir, "bin");
    mkdirSync(bin);
    mkdirSync(join(dir, "tmp", "cyberctf-e2e.Ab12Cd"), { recursive: true });
    mkdirSync(join(dir, "home"));
    writeFileSync(join(bin, "docker"), `#!/bin/sh\n${docker}\n`);
    chmodSync(join(bin, "docker"), 0o755);
    const env = {
      ...process.env,
      PATH: `${bin}:${process.env.PATH ?? ""}`,
      HOME: join(dir, "home"),
      TMPDIR: join(dir, "tmp"),
      XDG_DATA_HOME: "",
      VAGRANT_HOME: "",
    };
    const args = [join(HERE, "snapshot.sh"), "--out", join(dir, "snap.json"), ...(only ? ["--only", only] : [])];
    const r = spawnSync("bash", args, { env, encoding: "utf8" });
    expect(r.status, r.stderr).toBe(0);
    return readSnapshot(readFileSync(join(dir, "snap.json"), "utf8"), "snap.json");
  }

  it("records every probe with a known kind and status, each readable by its parser", () => {
    const snap = snapshot("exit 1");
    expect(snap.os).toMatch(/^(linux|macos)$/);
    for (const p of snap.probes) {
      expect(Object.keys(PARSERS)).toContain(p.kind);
      expect(["ok", "missing", "denied", "timeout", "failed"]).toContain(p.status);
      if (p.status === "ok") expect(() => parseProbe(p, snap.taken)).not.toThrow();
    }
    // Docker not answering: its kinds missing, saying so; the run's temp folder found.
    expect(snap.probes.find((p) => p.kind === "docker-container")).toMatchObject({
      status: "missing",
      note: "Docker isn't running (docker version has no server)",
    });
    expect(
      parseProbe(snap.probes.find((p) => p.kind === "temp-folder") ?? { kind: "temp-folder", tool: "", status: "", note: "", output: "" }, 0).map((i) =>
        i.name.split("/").pop(),
      ),
    ).toEqual(["cyberctf-e2e.Ab12Cd"]);
    // A whole machine's snapshot: a few seconds alone, more beside the other test files.
  }, 90_000);

  it("keeps a tool's JSON whole through its own escaping: quotes, backslashes, tabs, line breaks", () => {
    const line = JSON.stringify({ id: "c1", name: '/we"ird\\name', created: "2026-11-11T12:00:00Z", labels: { "a\tb": "x" }, state: "running" });
    const snap = snapshot(
      `case "$*" in
  version*) echo 29.0 ;;
  "ps -aq --no-trunc") echo c1 ;;
  inspect*) printf '%s\\n' '${line.replaceAll("'", "'\\''")}' ;;
  *) ;;
esac`,
      "docker",
    );
    const probe = snap.probes.find((p) => p.kind === "docker-container");
    expect(probe?.status).toBe("ok");
    expect(parseProbe(probe ?? { kind: "", tool: "", status: "", note: "", output: "" }, 0)[0]).toMatchObject({ key: "c1", name: 'we"ird\\name' });
    // --only docker: Docker's kinds alone.
    expect(snap.probes.map((p) => p.kind)).toEqual(["docker-container", "docker-network", "docker-volume", "docker-image"]);
  });
});
