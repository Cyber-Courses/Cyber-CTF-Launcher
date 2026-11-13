import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  boxLabel,
  dropComposeRecord,
  isUnder,
  leftoverIsTheRuns,
  ownedDownloads,
  ownedProjects,
  projectVolumes,
  removeDangling,
  parseBoxList,
  parseComposeLs,
  parseImageIds,
  projects,
  readBefore,
  record,
  runContainers,
  runFolders,
  runRoot,
} from "./lib/owned.mjs";

// What a desktop run owns (lib/owned.mjs): what its clean-ups may remove, decided from what the
// run wrote, never from a name. The machine is scripted: `tools` answers each command.

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** A run's folder with `labs` installed in it. */
function run(labs: string[] = []): string {
  const root = mkdtempSync(join(tmpdir(), "cyberctf-e2e-test-"));
  dirs.push(root);
  for (const id of labs) mkdirSync(join(runFolders(root).labs, id), { recursive: true });
  return root;
}

/** The machine as `answers` script it, by `program arg…`: unscripted commands fail (null). */
function tools(answers: Record<string, string | null>) {
  const calls: string[] = [];
  const fn = (program: string, args: string[]) => {
    const line = [program, ...args].join(" ");
    calls.push(line);
    const key = Object.keys(answers).find((k) => line.startsWith(k));
    return key === undefined ? null : answers[key];
  };
  return Object.assign(fn, { calls });
}

const composeLs = (projects: { Name: string; ConfigFiles: string }[]) => JSON.stringify(projects.map((p) => ({ ...p, Status: "running(2)" })));

describe("a path in the run's folder", () => {
  it("is the folder or below it, never a sibling that shares its start", () => {
    expect(isUnder("/tmp/cyberctf-e2e.ab/app/labs/x/compose.yml", "/tmp/cyberctf-e2e.ab", "darwin")).toBe(true);
    expect(isUnder("/tmp/cyberctf-e2e.ab", "/tmp/cyberctf-e2e.ab/", "linux")).toBe(true);
    expect(isUnder("/tmp/cyberctf-e2e.abc/app/compose.yml", "/tmp/cyberctf-e2e.ab", "linux")).toBe(false);
    expect(isUnder("/Users/me/Library/Application Support/org.cyberctf.desktop/labs/x/compose.yml", "/tmp/cyberctf-e2e.ab", "darwin")).toBe(false);
    expect(isUnder("/x", "", "linux")).toBe(false);
  });

  it("on Windows, whatever the case and the separators", () => {
    const root = String.raw`C:\Users\Me\AppData\Local\Temp\cyberctf-e2e-abc`;
    expect(isUnder(String.raw`c:\users\me\appdata\local\temp\CYBERCTF-E2E-ABC\app\labs\x\compose.yml`, root, "win32")).toBe(true);
    expect(isUnder("C:/Users/Me/AppData/Local/Temp/cyberctf-e2e-abc/app/compose.yml", root, "win32")).toBe(true);
    expect(isUnder(String.raw`C:\Users\Me\AppData\Roaming\org.cyberctf.desktop\labs\x\compose.yml`, root, "win32")).toBe(false);
    // Elsewhere case matters.
    expect(isUnder("/TMP/run/app", "/tmp/run", "linux")).toBe(false);
  });
});

/** Writes the app's record of run `name` in `root`: `record` as JSON, or as it is when a string. */
function runRecord(root: string, name: string, record: Record<string, unknown> | string): string {
  const dir = join(runFolders(root).runs, name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "record.json"), typeof record === "string" ? record : JSON.stringify({ version: 0, name, ...record }));
  writeFileSync(join(dir, "ports.yml"), "services: {}\n");
  return dir;
}

describe("the app's record of a run the clean-up took down", () => {
  it("goes when it is a Compose run on this machine of a lab installed in the run's folder", () => {
    const root = run(["local-lab"]);
    const dir = runRecord(root, "cyberctf-local-lab", { target: "docker", driver: "compose", state: "ready" });
    expect(dropComposeRecord(root, "cyberctf-local-lab")).toBe(true);
    expect(existsSync(dir)).toBe(false);
    // The lab stays installed: it is what makes its project the run's.
    expect(existsSync(join(runFolders(root).labs, "local-lab"))).toBe(true);
    expect(dropComposeRecord(root, "cyberctf-local-lab")).toBe(false);
  });

  it("stays for anything else: a VM's run, a server's, a lab not the run's, a record that doesn't read", () => {
    const root = run(["vm-lab", "server-lab", "docker-vm-lab", "odd-lab", "local-lab"]);
    const kept = [
      runRecord(root, "cyberctf-vm-lab", { target: "vagrant", driver: "vagrant", state: "ready" }),
      runRecord(root, "cyberctf-server-lab", { target: "docker", driver: "compose", host_label: "pve1", state: "ready" }),
      runRecord(root, "cyberctf-docker-vm-lab", { target: "docker-vm", driver: "compose", state: "ready" }),
      runRecord(root, "cyberctf-odd-lab", "not json"),
      runRecord(root, "cyberctf-other-lab", { target: "docker", driver: "compose", state: "ready" }),
    ];
    for (const name of [
      "cyberctf-vm-lab",
      "cyberctf-server-lab",
      "cyberctf-docker-vm-lab",
      "cyberctf-odd-lab",
      "cyberctf-other-lab",
      "../app",
      "cyberctf-../labs",
    ])
      expect(dropComposeRecord(root, name), name).toBe(false);
    for (const dir of kept) expect(existsSync(dir), dir).toBe(true);
    // A record of another run's name in this one's folder.
    runRecord(root, "cyberctf-local-lab", { name: "cyberctf-elsewhere", target: "docker", driver: "compose" });
    expect(dropComposeRecord(root, "cyberctf-local-lab")).toBe(false);
  });
});

describe("the run's Compose projects", () => {
  it("are its labs' projects started from its folder; a project of the same name started elsewhere is the user's", () => {
    const root = "/tmp/cyberctf-e2e.ab";
    const listed = parseComposeLs(
      composeLs([
        { Name: "cyberctf-local-invoice-portal-api", ConfigFiles: `${root}/app/labs/local-invoice-portal-api/.isoloom-1/compose.yml` },
        // The user's own copy of the same catalogue lab: the same name, its files in their data folder.
        { Name: "cyberctf-supplier-portal", ConfigFiles: "/Users/me/Library/Application Support/org.cyberctf.desktop/labs/supplier-portal/compose.yml" },
        // Partly from elsewhere: not the run's alone.
        { Name: "cyberctf-mixed", ConfigFiles: `${root}/app/labs/mixed/compose.yml,/home/me/mixed/compose.yml` },
        // From the run's folder but no lab of the run's: a name the run never made.
        { Name: "cyberctf-stray", ConfigFiles: `${root}/elsewhere/compose.yml` },
        { Name: "my-project", ConfigFiles: "/home/me/p/compose.yml" },
      ]),
    );
    const owned = ownedProjects(listed, { root, labIds: ["local-invoice-portal-api", "supplier-portal", "mixed"] }, "darwin");
    expect(owned).toEqual({
      ours: ["cyberctf-local-invoice-portal-api"],
      theirs: ["cyberctf-supplier-portal", "cyberctf-mixed", "cyberctf-stray", "my-project"],
    });
  });

  it("are none when Docker can't say, or says nothing readable", () => {
    expect(parseComposeLs("")).toEqual([]);
    expect(parseComposeLs("not json")).toEqual([]);
    expect(parseComposeLs('{"Name":"x"}')).toEqual([]);
    expect(parseComposeLs('[{"Name":"x"}]')).toEqual([{ name: "x", configFiles: [] }]);
    // A project with no Compose file named is never the run's.
    expect(ownedProjects([{ name: "cyberctf-a", configFiles: [] }], { root: "/r", labIds: ["a"] }, "linux").ours).toEqual([]);
  });

  it("are read from the labs installed in the run's folder, and their containers by the project's label", () => {
    const root = run(["local-a", "b"]);
    const sh = tools({
      "docker compose ls": composeLs([
        { Name: "cyberctf-local-a", ConfigFiles: join(root, "app", "labs", "local-a", "compose.yml") },
        { Name: "cyberctf-b", ConfigFiles: "/elsewhere/b/compose.yml" },
      ]),
      "docker ps": "cyberctf-local-a-web-1\ncyberctf-local-a-attackbox-1\n",
    });
    expect(projects(root, sh)).toEqual({ ours: ["cyberctf-local-a"], theirs: ["cyberctf-b"] });
    expect(runContainers(root, sh)).toEqual(["cyberctf-local-a-web-1", "cyberctf-local-a-attackbox-1"]);
    expect(sh.calls).toContain("docker ps -a --format {{.Names}} --filter label=com.docker.compose.project=cyberctf-local-a");
    expect(sh.calls.some((c) => c.includes("cyberctf-b"))).toBe(false);
  });
});

describe("a lab already running before a spec starts it", () => {
  it("is stopped only when the run started one and no lab of the user's runs", () => {
    const root = run(["a"]);
    const ours = composeLs([{ Name: "cyberctf-a", ConfigFiles: join(root, "app", "labs", "a", "compose.yml") }]);
    const theirs = { Name: "cyberctf-a", ConfigFiles: "/home/me/.local/share/org.cyberctf.desktop/labs/a/compose.yml" };
    expect(leftoverIsTheRuns(root, tools({ "docker compose ls": ours })).ok).toBe(true);
    const users = leftoverIsTheRuns(root, tools({ "docker compose ls": composeLs([theirs]) }));
    expect(users).toEqual({ ok: false, why: "labs this run didn't start run here (cyberctf-a), left as they are" });
    expect(leftoverIsTheRuns(root, tools({ "docker compose ls": "[]" })).ok).toBe(false);
    expect(leftoverIsTheRuns(null, tools({})).ok).toBe(false);
    // A VM the run's app made (Vagrant's machine folder in its lab).
    mkdirSync(join(root, "app", "labs", "a", "vagrant", ".vagrant", "machines", "default"), { recursive: true });
    expect(leftoverIsTheRuns(root, tools({ "docker compose ls": "[]" })).ok).toBe(true);
  });
});

describe("the run's downloads", () => {
  const before = { images: new Set(["sha256:kali-had"]), boxes: new Set([boxLabel("bento/debian-12", "virtualbox", "1")]) };
  const ids: Record<string, string> = { "nginx:1.27": "sha256:nginx", "cyberctf/attack-box:latest": "sha256:kali-had" };
  const imageId = (r: string) => ids[r] ?? null;

  it("are the images and boxes that weren't here at the start, and the plugins in the run's folder", () => {
    const listed = {
      images: [{ name: "nginx:1.27" }, { name: "cyberctf/attack-box:latest" }, { name: "gone:1" }],
      boxes: [{ name: "generic/alpine319 (virtualbox, 4.3.12)" }, { name: "bento/debian-12 (virtualbox, 1)" }],
      plugins: [{ name: "Terraform provider plugins" }],
    };
    expect(ownedDownloads(listed, before, imageId)).toEqual({
      ours: ["nginx:1.27", "generic/alpine319 (virtualbox, 4.3.12)", "Terraform provider plugins"],
      // The attack box's image the user had before (the app lists it by name), and one Docker can't name now.
      theirs: ["cyberctf/attack-box:latest", "gone:1", "bento/debian-12 (virtualbox, 1)"],
    });
  });

  it("are none of a kind whose start wasn't recorded", () => {
    const listed = { images: [{ name: "nginx:1.27" }], boxes: [{ name: "a/b (qemu, 1)" }], plugins: [] };
    expect(ownedDownloads(listed, { images: null, boxes: null }, imageId)).toEqual({ ours: [], theirs: ["nginx:1.27", "a/b (qemu, 1)"] });
    expect(ownedDownloads(undefined, before, imageId)).toEqual({ ours: [], theirs: [] });
  });

  it("read Vagrant's list as the app names a box, an architecture left out", () => {
    const list = "bento/debian-12   (virtualbox, 202510.26.0)\nutm/bookworm (utm, 0.0.1, (arm64))\r\nThere are no installed boxes!\n";
    expect(parseBoxList(list)).toEqual(new Set(["bento/debian-12 (virtualbox, 202510.26.0)", "utm/bookworm (utm, 0.0.1)"]));
    expect(parseImageIds("sha256:a\r\n\nsha256:b\n")).toEqual(new Set(["sha256:a", "sha256:b"]));
  });

  it("are told apart by what the run recorded at its start: each kind only when its tool could say", () => {
    const root = run();
    record(root, tools({ "docker image ls": "sha256:a\nsha256:b\n", "vagrant box list": null }));
    expect(readFileSync(join(root, "records", "images.txt"), "utf8")).toBe("sha256:a\nsha256:b\n");
    expect(readBefore(root)).toEqual({ images: new Set(["sha256:a", "sha256:b"]), boxes: null });
    // Not installed: nothing of it here then.
    const other = run();
    record(other, tools({ "docker image ls": null, "vagrant box list": "" }));
    expect(readBefore(other)).toEqual({ images: null, boxes: new Set() });
  });
});

describe("the command line", () => {
  it("lists nothing as the run's for a run with no lab, and refuses a missing folder", () => {
    const root = run();
    const script = join(import.meta.dirname, "lib", "owned.mjs");
    // Whatever Docker runs here, none of it is the run's: the run installed no lab.
    const r = spawnSync(process.execPath, [script, "projects", root], { encoding: "utf8" });
    expect(r.status).toBe(0);
    expect(r.stdout).not.toMatch(/^ours/m);
    expect(spawnSync(process.execPath, [script, "projects"], { encoding: "utf8" }).status).toBe(2);
  });

  it("finds the run's folder in CYBERCTF_WDIO_ROOT", () => {
    expect(runRoot({ CYBERCTF_WDIO_ROOT: "/tmp/run" })).toBe("/tmp/run");
    expect(runRoot({})).toBeNull();
  });
});

describe("the volumes a project's containers hold", () => {
  const A = "e465cfbd5789f9aff3cbbc62398bb3830e0f74b37b835640ad9d85b7a9dcbae1";
  const B = "135385800a74aaa45a61d725cd8c13a7f4cb47a4822ee6f28defae9b133c1b08";

  it("are read from the containers by the project's label, each once", () => {
    const sh = tools({ "docker ps -aq": "c1\nc2\n", "docker inspect": `${A}\n${B}\n${A}\n` });
    expect(projectVolumes("cyberctf-local-a", sh)).toEqual([A, B]);
    expect(sh.calls[0]).toBe("docker ps -aq --filter label=com.docker.compose.project=cyberctf-local-a");
    expect(sh.calls[1].endsWith(" c1 c2")).toBe(true);
  });

  it("are none without a container, and none when the engine doesn't answer", () => {
    expect(projectVolumes("cyberctf-local-a", tools({ "docker ps -aq": "" }))).toEqual([]);
    expect(projectVolumes("cyberctf-local-a", tools({}))).toEqual([]);
  });

  it("are removed after the down only where the engine holds them dangling", () => {
    const sh = tools({ "docker volume ls": `${B}\nother\n`, "docker volume rm": "" });
    expect(removeDangling([A, B], sh)).toEqual([B]);
    expect(sh.calls).toContain(`docker volume rm --force ${B}`);
    expect(removeDangling([], sh)).toEqual([]);
    expect(removeDangling([A], tools({ "docker volume ls": "" }))).toEqual([]);
  });
});
