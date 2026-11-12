import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { PARSERS, parseProbe, readSnapshot } from "./inventory.ts";
import { KINDS } from "./kinds.ts";
import { globRegExp } from "./scope.ts";

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
   *  --only groups; `tmpLink`: TMPDIR a symlink to the temp folder, as macOS's /tmp is). The
   *  snapshot, and the TMPDIR it was given. */
  function snapshot(docker: string, { only = "", tmpLink = false } = {}) {
    const dir = mkdtempSync(join(tmpdir(), "audit-snapshot-test-"));
    dirs.push(dir);
    const bin = join(dir, "bin");
    mkdirSync(bin);
    mkdirSync(join(dir, "tmp", "cyberctf-e2e.Ab12Cd"), { recursive: true });
    mkdirSync(join(dir, "home"));
    writeFileSync(join(bin, "docker"), `#!/bin/sh\n${docker}\n`);
    chmodSync(join(bin, "docker"), 0o755);
    const tmp = tmpLink ? join(dir, "tmp-link") : join(dir, "tmp");
    if (tmpLink) symlinkSync(join(dir, "tmp"), tmp);
    const env = {
      ...process.env,
      PATH: `${bin}:${process.env.PATH ?? ""}`,
      HOME: join(dir, "home"),
      TMPDIR: tmp,
      XDG_DATA_HOME: "",
      VAGRANT_HOME: "",
    };
    const args = [join(HERE, "snapshot.sh"), "--out", join(dir, "snap.json"), ...(only ? ["--only", only] : [])];
    const r = spawnSync("bash", args, { env, encoding: "utf8" });
    expect(r.status, r.stderr).toBe(0);
    return { snap: readSnapshot(readFileSync(join(dir, "snap.json"), "utf8"), "snap.json"), tmp };
  }

  /** The temp-folder probe's entries in `tmp`, by name. snapshot.sh looks in /tmp and /var/tmp
   *  too, whatever TMPDIR says, so what other runs left there (a CI runner's
   *  /tmp/cyberctf-tests-*, scripts/check/temp-leaks.ts) is left out. */
  const tempFolders = (snap: ReturnType<typeof snapshot>["snap"], tmp: string) =>
    parseProbe(snap.probes.find((p) => p.kind === "temp-folder") ?? { kind: "temp-folder", tool: "", status: "", note: "", output: "" }, 0)
      .map((i) => i.name)
      .filter((name) => name.startsWith(`${tmp}/`))
      .map((name) => name.slice(tmp.length + 1));

  it("records every probe with a known kind and status, each readable by its parser", () => {
    const { snap, tmp } = snapshot("exit 1");
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
    expect(tempFolders(snap, tmp)).toEqual(["cyberctf-e2e.Ab12Cd"]);
    // A whole machine's snapshot: a few seconds alone, more beside the other test files.
  }, 90_000);

  it("looks into a temp folder that is a symlink, as macOS's /tmp is", () => {
    const { snap, tmp } = snapshot("exit 1", { only: "files", tmpLink: true });
    expect(tempFolders(snap, tmp)).toEqual(["cyberctf-e2e.Ab12Cd"]);
  });

  it("keeps a tool's JSON whole through its own escaping: quotes, backslashes, tabs, line breaks", () => {
    const line = JSON.stringify({ id: "c1", name: '/we"ird\\name', created: "2026-11-11T12:00:00Z", labels: { "a\tb": "x" }, state: "running" });
    const snap = snapshot(
      `case "$*" in
  version*) echo 29.0 ;;
  "ps -aq --no-trunc") echo c1 ;;
  inspect*) printf '%s\\n' '${line.replaceAll("'", "'\\''")}' ;;
  *) ;;
esac`,
      { only: "docker" },
    ).snap;
    const probe = snap.probes.find((p) => p.kind === "docker-container");
    expect(probe?.status).toBe("ok");
    expect(parseProbe(probe ?? { kind: "", tool: "", status: "", note: "", output: "" }, 0)[0]).toMatchObject({ key: "c1", name: 'we"ird\\name' });
    // --only docker: Docker's kinds alone.
    expect(snap.probes.map((p) => p.kind)).toEqual(["docker-container", "docker-network", "docker-volume", "docker-image"]);
  });
});

describe("the stack's processes (snapshot.ps1's $StackProcesses, snapshot.sh's STACK_PROCESSES)", () => {
  // A machine whose account is `vagrant` (a Vagrant box: the rig's Windows and Linux guests) runs
  // everything under C:\Users\vagrant\ or /home/vagrant/: Vagrant is the program, never a word of a
  // path. Each pattern as its script applies it: PowerShell's -match (case-insensitive) on
  // "<name> <command line>", grep -E on ps's "<pid> <etime> <command>".
  const windows = /\$StackProcesses = '([^']+)'/.exec(read("snapshot.ps1"))?.[1] ?? "";
  const unix = /^STACK_PROCESSES='([^']+)'/m.exec(read("snapshot.sh"))?.[1] ?? "";
  const VAGRANT_WINDOWS = [
    'vagrant.exe "C:\\Program Files\\Vagrant\\bin\\vagrant.exe" up --provider hyperv',
    'ruby.exe "C:\\Program Files\\Vagrant\\embedded\\mingw64\\bin\\ruby.exe" "C:\\Program Files\\Vagrant\\embedded\\gems\\gems\\vagrant-2.4.9\\bin\\vagrant" destroy -f',
    'cyber-ctf-desktop.exe "C:\\Program Files\\Cyber CTF\\cyber-ctf-desktop.exe" deploy --job C:\\Users\\vagrant\\AppData\\Roaming\\org.cyberctf.desktop\\deploys\\a.json',
  ];
  const NOT_WINDOWS = [
    'OneDrive.exe "C:\\Users\\vagrant\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe" /background',
    'Docker Desktop.exe "C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe" --name=gui --user-data-dir="C:\\Users\\vagrant\\AppData\\Roaming\\Docker Desktop"',
    'pwsh.exe "C:\\Users\\vagrant\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe" -NoProfile -File C:\\Users\\vagrant\\rig\\job.ps1',
  ];
  const VAGRANT_UNIX = [
    " 4100    1:15:00 vagrant ssh --command docker compose --project-name cyberctf-hello up",
    "  200      10:00 /opt/vagrant/embedded/bin/ruby /opt/vagrant/embedded/gems/gems/vagrant-2.4.9/bin/vagrant up --provider qemu",
    "  201   01-02:03:04 /usr/bin/vagrant destroy -f",
  ];
  const NOT_UNIX = [
    "  300      10:00 /usr/bin/python3 /home/vagrant/rig/agent.py --work /home/vagrant/work",
    "  301      10:00 sshd: vagrant@pts/0",
    "  302      10:00 sudo -u vagrant -i",
  ];

  it("are read from both scripts", () => {
    expect(windows).not.toBe("");
    expect(unix).not.toBe("");
  });

  it("take Vagrant by its program on Windows, never a path under C:\\Users\\vagrant\\", () => {
    const re = new RegExp(windows, "i");
    for (const line of VAGRANT_WINDOWS) expect(re.test(line), line).toBe(true);
    for (const line of NOT_WINDOWS) expect(re.test(line), line).toBe(false);
  });

  it("do so in PowerShell itself, where it's installed", () => {
    if (spawnSync("pwsh", ["-NoProfile", "-Command", "exit 0"]).status !== 0) return;
    const lines = [...VAGRANT_WINDOWS, ...NOT_WINDOWS];
    const script = `$p = '${windows.replaceAll("'", "''")}'; foreach ($l in $input) { if ($l -match $p) { 'yes' } else { 'no' } }`;
    const r = spawnSync("pwsh", ["-NoProfile", "-Command", script], { input: lines.join("\n"), encoding: "utf8" });
    expect(r.stdout.trim().split(/\r?\n/)).toEqual([...VAGRANT_WINDOWS.map(() => "yes"), ...NOT_WINDOWS.map(() => "no")]);
  });

  it("take Vagrant by its program in ps's lines, never a path under /home/vagrant/ (grep -E, as the script)", () => {
    const r = spawnSync("grep", ["-E", unix], { input: [...VAGRANT_UNIX, ...NOT_UNIX].join("\n") + "\n", encoding: "utf8" });
    expect(r.stdout.trimEnd().split("\n")).toEqual(VAGRANT_UNIX);
  });
});

describe("what the guided setup leaves (setup-leaves.tsv)", () => {
  const rows = read("setup-leaves.tsv")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => {
      const [item = "", os = "", kind = "", name = "", reason = ""] = l.split("\t");
      return { item, os, kind, name, reason };
    });

  it("names a kind of the listing on a system that reads it, a name and a reason, a row each", () => {
    expect(rows.length).toBeGreaterThan(40);
    for (const r of rows) {
      const kind = KINDS.find((k) => k.kind === r.kind);
      expect(kind, `${r.item}: ${r.kind}`).toBeDefined();
      expect(["linux", "macos", "windows", "any"], `${r.item}: ${r.os}`).toContain(r.os);
      if (r.os !== "any") expect(kind?.on, `${r.item}: ${r.kind} isn't read on ${r.os}`).toContain(r.os);
      expect(r.name, r.item).not.toBe("");
      expect(r.reason, r.item).not.toBe("");
    }
    // What Docker Engine 29 leaves on Linux besides its own chains (ubuntu2404-arm64-bare's
    // setup-docker audit): iptables-nft's raw table and its base chains.
    const docker = rows.filter((r) => r.item === "docker" && r.os === "linux");
    for (const [kind, name] of [
      ["nft-table", "table ip raw"],
      ["iptables-rule", "raw :PREROUTING ACCEPT"],
      ["iptables-rule", "raw :OUTPUT ACCEPT"],
      ["nft-table", "table ip nat"],
      ["iptables-rule", "filter :FORWARD DROP"],
    ])
      expect(
        docker.some((r) => r.kind === kind && globRegExp(r.name).test(name)),
        `${kind} ${name}`,
      ).toBe(true);
    // The paths the setup sets up each leave something: the engine, each hypervisor, the launcher.
    for (const item of ["launcher", "docker", "libvirt", "qemu", "virtualbox", "hyperv", "wsl"])
      expect(
        rows.some((r) => r.item === item),
        item,
      ).toBe(true);
  });
});
