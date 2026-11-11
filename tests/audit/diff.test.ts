import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { diff, nameList, report, toRecord } from "./diff.ts";
import type { Probe, Snapshot } from "./inventory.ts";
import { ALLOWLIST, isMain, parseDiffArgs, runDiff } from "./main.ts";
import { type Allowance, type Scope, allowanceFor, globRegExp, mentions, parseWindow, projectOf, readAllowlist, readLogs, under } from "./scope.ts";

// The leftover audit's diff: what's new sorted into what the run left, what it may leave and what
// isn't its. On a machine someone uses (the owner's Mac), the owner's own labs, VMs, containers
// and files exist before, during and after the run, and some are made while it runs: none of them
// may ever be reported as the run's.

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});
function folder(): string {
  const d = mkdtempSync(join(tmpdir(), "audit-test-"));
  dirs.push(d);
  return d;
}

const ok = (kind: string, output: string): Probe => ({ kind, tool: "t", status: "ok", note: "", output });
const snap = (taken: number, probes: Probe[]): Snapshot => ({ audit: 1, os: "macos", host: "mac", user: "owner", taken, roots: [], probes });
const iso = (s: number) => new Date(s * 1000).toISOString();
const container = (id: string, name: string, project: string, created: number, workdir = "") =>
  JSON.stringify({
    id,
    name: `/${name}`,
    created: iso(created),
    labels: { "com.docker.compose.project": project, ...(workdir ? { "com.docker.compose.project.working_dir": workdir } : {}) },
    state: "running",
  });
const allowlist = readAllowlist(readFileSync(ALLOWLIST, "utf8"), ALLOWLIST);
const scope = (s: Partial<Scope>): Scope => ({ mode: "shared", roots: [], names: [], logText: "", windows: [], ...s });
const leftNames = (results: ReturnType<typeof diff>) =>
  Object.fromEntries(results.filter((r) => r.left.length > 0).map((r) => [r.kind, r.left.map((i) => i.name)]));

// The owner's Mac: before the run, their own container and lab, their temp folders and VM.
const OWNER_BEFORE = [
  container("own1", "postgres", "own", 100),
  container("own2", "cyberctf-mylab-web-1", "cyberctf-mylab", 200, "/Users/owner/Library/Application Support/org.cyberctf.desktop/labs/mylab"),
].join("\n");
const ROOT = "/var/folders/xy/T/cyberctf-e2e.Ab12Cd";

// After: the run's container (named in its logs, made in its window), the owner's new lab made
// meanwhile (not named), a container of the run's project made hours after the run by another
// process, the run's root's leftover and the owner's own.
const AFTER_CONTAINERS = [
  OWNER_BEFORE,
  container("run1", "cyberctf-hello-web-1", "cyberctf-hello", 1500),
  container("own3", "cyberctf-scratch-db-1", "cyberctf-scratch", 1600),
  container("late", "cyberctf-hello-db-1", "cyberctf-hello", 5000),
  container("e2e1", "cyberctf-fixture-web-1", "cyberctf-fixture", 1700, `${ROOT}/app/labs/fixture/.isoloom/docker`),
].join("\n");

const before = snap(900, [
  ok("docker-container", OWNER_BEFORE),
  ok("docker-image", ""),
  ok("temp-folder", "/var/folders/xy/T/cyberctf-box-1-home"),
  ok("utm-vm", "UUID Status Name\n"),
  ok("process", "  700 10-00:00:00 /Applications/Cyber CTF.app/Contents/MacOS/cyber-ctf-desktop\nnow 900\n"),
  ok("listening-port", "p700\nccyber-ctf\nn127.0.0.1:47290\n"),
  { kind: "parallels-vm", tool: "prlctl", status: "missing", note: "no prlctl on PATH", output: "" },
]);
const after = snap(6000, [
  ok("docker-container", AFTER_CONTAINERS),
  ok("docker-image", JSON.stringify({ ID: "sha256:aa", Repository: "busybox", Tag: "1.37.0-musl", CreatedAt: "2025-10-08 20:12:01 +0000 UTC" })),
  ok("temp-folder", ["/var/folders/xy/T/cyberctf-box-1-home", "/var/folders/xy/T/cyberctf-box-2-home", ROOT].join("\n")),
  ok("utm-vm", "UUID                                 Status   Name\nA1B2C3D4-0000-4000-8000-000000000002 stopped  Owner Windows 11\n"),
  ok(
    "process",
    [
      "  700 10-01:23:20 /Applications/Cyber CTF.app/Contents/MacOS/cyber-ctf-desktop",
      " 4100    1:15:00 vagrant ssh --command docker compose --project-name cyberctf-hello up",
      " 4200    0:10:00 /Applications/Cyber CTF.app/Contents/MacOS/cyber-ctf-desktop deploy --job /Users/owner/Library/Application Support/org.cyberctf.desktop/deploys/mylab.json",
      "now 6000",
    ].join("\n"),
  ),
  ok("listening-port", "p700\nccyber-ctf\nn127.0.0.1:47290\np4100\ncssh\nn127.0.0.1:50022\np4200\ncssh\nn127.0.0.1:50023\n"),
  { kind: "parallels-vm", tool: "prlctl", status: "missing", note: "no prlctl on PATH", output: "" },
]);
// The run's logs name its project and its root; one pass ran from 1000 to 2000.
const RUN_LOGS = `docker compose --project-name cyberctf-hello up\nroot: ${ROOT}\n`;

describe("on a machine someone uses (shared)", () => {
  const results = diff(before, after, scope({ logText: RUN_LOGS, windows: [{ start: 1000, end: 2000 }], roots: [ROOT] }), allowlist);

  it("reports only what the run proves it made: named in its logs and made in its window, or under its root", () => {
    expect(leftNames(results)).toEqual({
      "docker-container": ["cyberctf-hello-web-1", "cyberctf-fixture-web-1"],
      "temp-folder": [ROOT],
      process: ["4100 vagrant ssh --command docker compose --project-name cyberctf-hello up"],
      "listening-port": ["127.0.0.1:50022 (ssh, pid 4100)"],
    });
  });

  it("leaves the owner's own alone: made meanwhile, of the same project after the run, a VM, a worker", () => {
    const others = Object.fromEntries(results.map((r) => [r.kind, r.others.map((i) => i.name)]));
    // Another process made a container of the run's project three hours on: not the run's.
    expect(others["docker-container"]).toEqual(["cyberctf-scratch-db-1", "cyberctf-hello-db-1"]);
    expect(others["temp-folder"]).toEqual(["/var/folders/xy/T/cyberctf-box-2-home"]);
    expect(others["utm-vm"]).toEqual(["Owner Windows 11"]);
    expect(others["listening-port"]).toEqual(["127.0.0.1:50023 (ssh, pid 4200)"]);
    // What existed before is never new, whatever its name.
    expect(results.flatMap((r) => [...r.left, ...r.others]).map((i) => i.name)).not.toContain("cyberctf-mylab-web-1");
  });

  it("allows the cache, saying why, and skips a kind its tool can't read", () => {
    const images = results.find((r) => r.kind === "docker-image");
    expect(images?.allowed.map((a) => a.item.name)).toEqual(["busybox:1.37.0-musl"]);
    expect(images?.reason).toContain("the launcher's cache");
    expect(results.find((r) => r.kind === "parallels-vm")).toMatchObject({ result: "skip", reason: "no prlctl on PATH (prlctl, before the run)" });
  });

  it("with no name, log or root of the run's, reports nothing at all: never a blanket diff", () => {
    expect(leftNames(diff(before, after, scope({}), allowlist))).toEqual({});
  });

  it("needs a pass's own logs to name a thing made in that pass's window", () => {
    // Two passes: the run's project named only in pass b's logs, its container made in pass a's.
    const windows = [
      { start: 1000, end: 1550, text: "pass a: nothing of hello" },
      { start: 1560, end: 2000, text: "pass b: --project-name cyberctf-hello" },
    ];
    const r = diff(before, after, scope({ windows }), allowlist);
    expect(leftNames(r)["docker-container"]).toBeUndefined();
    const r2 = diff(before, after, scope({ windows: [{ start: 1000, end: 2000, text: "--project-name cyberctf-hello" }] }), allowlist);
    expect(leftNames(r2)["docker-container"]).toEqual(["cyberctf-hello-web-1"]);
  });

  it("takes a name it was given as the run's", () => {
    const r = diff(before, after, scope({ names: ["cyberctf-scratch"] }), allowlist);
    expect(leftNames(r)["docker-container"]).toEqual(["cyberctf-scratch-db-1"]);
  });
});

describe("jobs side by side on one machine (shared)", () => {
  // The rig's docker-vm-lab@qemu job, beside a launcher test that ran a lab with a `database` VM
  // and another docker-vm job: Vagrant's index lists all three. The job's logs name "database"
  // (apt's "Reading database …") and "default" (Vagrant's own machine), never the others' folders.
  const SUPPORT = "/Users/owner/Library/Application Support";
  const machine = (name: string, dir: string) => ({ name, provider: "qemu", vagrantfile_path: dir, local_data_path: `${dir}/.vagrant` });
  const index = (machines: Record<string, object>) => JSON.stringify({ version: 1, machines });
  const others = {
    db: machine("database", `${SUPPORT}/com.cyberctf.unit-test.19990-a1/labs/pm-9221/.isoloom/vagrant`),
    dvm: machine("default", "/var/folders/xy/T/cyberctf-dvm-21666/.isoloom/docker-vm"),
  };
  const own = { mine: machine("default", "/var/folders/xy/T/cyberctf-dvm-30500/.isoloom/docker-vm") };
  const LOGS = "Reading database ... 34%\nBringing machine 'default' up with 'qemu' provider...\nlab: /var/folders/xy/T/cyberctf-dvm-30500\n";
  const vagrant = (machines: Record<string, object>) => snap(6000, [ok("vagrant-machine", index(machines))]);
  const left = (machines: Record<string, object>, logText: string) =>
    leftNames(diff(snap(900, [ok("vagrant-machine", index({}))]), vagrant(machines), scope({ logText, windows: [{ start: 1000, end: 2000 }] }), allowlist));

  it("takes a machine of a lab's folder as the run's only when the run names that folder, never by its machine's name", () => {
    expect(left({ ...others, ...own }, LOGS)).toEqual({
      "vagrant-machine": ["default (qemu, /var/folders/xy/T/cyberctf-dvm-30500/.isoloom/docker-vm)"],
    });
    expect(left(others, LOGS)).toEqual({});
    // The other job's folder named by this run's logs: the run's after all.
    expect(left(others, `${LOGS}removed ${SUPPORT}/com.cyberctf.unit-test.19990-a1/labs/pm-9221\n`)["vagrant-machine"]).toEqual([
      `database (qemu, ${SUPPORT}/com.cyberctf.unit-test.19990-a1/labs/pm-9221/.isoloom/vagrant)`,
    ]);
  });

  it("knows a path's project folder, and a name only as a whole", () => {
    expect(projectOf(`${SUPPORT}/x/labs/pm-9221/.isoloom/vagrant/.vagrant`)).toBe("pm-9221");
    expect(projectOf("C:\\Users\\p\\own\\.vagrant")).toBe("own");
    expect(projectOf("/var/lib/libvirt/images/solo-web_web.img")).toBeUndefined();
    expect(mentions("lab pm-9221 up", "pm-9221")).toBe(true);
    expect(mentions("--project-name cyberctf-pm-9221", "pm-9221")).toBe(true);
    expect(mentions("lab pm-92210 up", "pm-9221")).toBe(false);
    expect(mentions("the databases", "database")).toBe(false);
    expect(mentions("a.b (x)", "a.b (x)")).toBe(true);
    expect(mentions("axb", "a.b")).toBe(false);
  });

  it("names a thing outside any lab's folder by its own names, as a whole", () => {
    const r = diff(before, after, scope({ logText: "docker compose --project-name cyberctf-hello2 up", windows: [{ start: 1000, end: 2000 }] }), allowlist);
    expect(leftNames(r)["docker-container"]).toBeUndefined();
  });
});

describe("on a throwaway clone", () => {
  it("reports everything new but what the allowlist lets a run leave", () => {
    const r = diff(before, after, scope({ mode: "clone" }), allowlist);
    expect(leftNames(r)["docker-container"]).toEqual(["cyberctf-hello-web-1", "cyberctf-scratch-db-1", "cyberctf-hello-db-1", "cyberctf-fixture-web-1"]);
    expect(leftNames(r)["docker-image"]).toBeUndefined();
    expect(r.find((x) => x.kind === "docker-container")).toMatchObject({
      result: "fail",
      reason: expect.stringMatching(/^left 4: cyberctf-hello-web-1, /) as unknown,
    });
  });

  it("skips a kind missing from either snapshot, or that the tool refused there", () => {
    const b = snap(1, [ok("vbox-vm", ""), { kind: "nft-table", tool: "nft", status: "denied", note: "needs root", output: "" }]);
    const a = snap(2, [ok("vbox-vm", '"x" {1}'), ok("nft-table", "table ip x"), ok("route", "")]);
    const r = diff(b, a, scope({ mode: "clone" }), []);
    expect(r.map((x) => [x.kind, x.result, x.reason])).toEqual([
      ["vbox-vm", "fail", "left 1: x"],
      ["nft-table", "skip", "needs root (nft, before the run)"],
      ["route", "skip", "not in the snapshot before the run"],
    ]);
  });
});

describe("the allowlist", () => {
  const entries: Allowance[] = readAllowlist("# c\nvbox-vm\tany\t!keep-*\tno\nvbox-vm\tany\t*\tthe cache\nroute\tclone\t10.*\ta clone's\n", "x");
  const item = (kind: string, name: string) => ({ kind, key: name, name, refs: [], paths: [] });

  it("lets the first matching entry decide, a `!` entry refusing", () => {
    expect(allowanceFor(item("vbox-vm", "box_1"), "shared", entries)?.reason).toBe("the cache");
    expect(allowanceFor(item("vbox-vm", "keep-me"), "shared", entries)).toBeUndefined();
    expect(allowanceFor(item("route", "10.0.0.0/8"), "clone", entries)?.reason).toBe("a clone's");
    expect(allowanceFor(item("route", "10.0.0.0/8"), "shared", entries)).toBeUndefined();
  });

  it("refuses an entry without a reason or a place", () => {
    expect(() => readAllowlist("vbox-vm\tany\t*\n", "f.tsv")).toThrow("f.tsv:1: an allowance is");
    expect(() => readAllowlist("vbox-vm\tsomewhere\t*\twhy\n", "f.tsv")).toThrow("f.tsv:1");
  });

  it("matches globs with classes, and the launcher's linked-clone masters and box volumes", () => {
    expect(globRegExp("a?[0-9]*").test("ab7xyz")).toBe(true);
    expect(globRegExp("a.b").test("axb")).toBe(false);
    expect(allowanceFor(item("vbox-vm", "debian-12.12-aarch64_1794132848692_83122"), "shared", allowlist)).toBeDefined();
    expect(allowanceFor(item("vbox-vm", "hello-stack - web"), "clone", allowlist)).toBeUndefined();
    expect(
      allowanceFor(item("libvirt-volume", "default/cloud-image-VAGRANTSLASH-debian-12_vagrant_box_image_2026_box.img (system)"), "clone", allowlist),
    ).toBeDefined();
    expect(allowanceFor(item("docker-volume", "cyberctf-hello_data (persistent)"), "clone", allowlist)).toBeDefined();
    expect(allowanceFor(item("docker-volume", "cyberctf-hello_data"), "clone", allowlist)).toBeUndefined();
    expect(allowanceFor(item("proxmox-token", "root@pam!cyberctf-1a2b3c4d"), "clone", allowlist)).toBeDefined();
  });
});

describe("the scope's inputs", () => {
  it("knows a path under a root, either separator", () => {
    expect(under("/a/b/c", "/a/b")).toBe(true);
    expect(under("/a/bc", "/a/b")).toBe(false);
    expect(under("C:\\r\\x", "C:/r/")).toBe(true);
    expect(under("/x", "")).toBe(false);
  });

  it("reads a run's logs whole, never the audit's own files", () => {
    const d = folder();
    writeFileSync(join(d, "integration.log"), "cyberctf-hello");
    writeFileSync(join(d, "audit-after.json"), "cyberctf-owner");
    expect(readLogs([d, join(d, "missing")])).toBe("cyberctf-hello");
  });

  it("parses a window, with its own logs", () => {
    const d = folder();
    writeFileSync(join(d, "a.log"), "pass a");
    expect(parseWindow("10:20")).toEqual({ start: 10, end: 20 });
    expect(parseWindow(`10:20=${join(d, "a.log")}`)).toEqual({ start: 10, end: 20, text: "pass a" });
    expect(() => parseWindow("yesterday")).toThrow("--window takes");
  });
});

describe("the records and the command", () => {
  it("writes one record of suite audit a kind, as the launcher's other entry points", () => {
    const [r] = diff(snap(1, [ok("vbox-vm", "")]), snap(2, [ok("vbox-vm", '"x" {1}')]), scope({ mode: "clone" }), []);
    expect(toRecord(r, "/r/audit-report.txt")).toEqual({
      suite: "audit",
      case: "vbox-vm",
      provider: "",
      form: "",
      result: "fail",
      reason: "left 1: x",
      seconds: 0,
      log: "/r/audit-report.txt",
    });
    expect(nameList(["a", "b", "c"], 2)).toBe("a, b and 1 more");
  });

  it("diffs two files, appends the records, writes the report and exits 1 on a leftover", () => {
    const d = folder();
    writeFileSync(join(d, "b.json"), JSON.stringify(before));
    writeFileSync(join(d, "a.json"), JSON.stringify(after));
    writeFileSync(join(d, "run.log"), RUN_LOGS);
    writeFileSync(join(d, "audit.jsonl"), '{"suite":"integration","case":"x","result":"pass"}\n');
    const args = parseDiffArgs([
      join(d, "b.json"),
      join(d, "a.json"),
      "--mode",
      "shared",
      "--named-in",
      join(d, "run.log"),
      "--window",
      "1000:2000",
      "--root",
      ROOT,
      "--results",
      join(d, "audit.jsonl"),
      "--report",
      join(d, "audit-report.txt"),
    ]);
    expect(runDiff(args)).toBe(1);
    const records = readFileSync(join(d, "audit.jsonl"), "utf8")
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as { case: string; result: string });
    expect(records[0].case).toBe("x");
    expect(records.filter((r) => r.result === "fail").map((r) => r.case)).toEqual(["docker-container", "temp-folder", "process", "listening-port"]);
    expect(readFileSync(join(d, "audit-report.txt"), "utf8")).toContain("    left:    cyberctf-hello-web-1");
  });

  it("knows it was run directly by any path to it, a link included", () => {
    const d = folder();
    const link = join(d, "main.ts");
    symlinkSync(join(import.meta.dirname, "main.ts"), link);
    expect(isMain(link, new URL("main.ts", import.meta.url).href)).toBe(true);
    expect(isMain(join(import.meta.dirname, "diff.ts"), new URL("main.ts", import.meta.url).href)).toBe(false);
    expect(isMain(undefined, import.meta.url)).toBe(false);
  });

  it("refuses arguments it doesn't know", () => {
    expect(() => parseDiffArgs(["a"])).toThrow("diff takes the snapshots before and after");
    expect(() => parseDiffArgs(["a", "b", "--mode", "everything"])).toThrow("--mode is clone or shared");
    expect(() => parseDiffArgs(["a", "b", "--bogus"])).toThrow("unknown option --bogus");
    expect(() => parseDiffArgs(["a", "b", "--root"])).toThrow("--root takes a value");
  });

  it("reports for a person what was left, allowed and not the run's", () => {
    const text = report(diff(before, after, scope({ logText: RUN_LOGS, roots: [ROOT] }), allowlist), before, after, scope({ roots: [ROOT] }));
    expect(text).toContain("docker-container     FAIL");
    expect(text).toContain("    allowed: busybox:1.37.0-musl (the images a lab pulls or builds stay");
    expect(text).toContain("    not the run's: Owner Windows 11");
    expect(text).toContain("parallels-vm         skip  no prlctl on PATH");
  });
});
