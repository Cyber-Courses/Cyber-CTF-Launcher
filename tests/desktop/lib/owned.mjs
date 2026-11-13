// What a desktop run owns, decided from what the run itself wrote, never from a name: a lab's
// Compose project is named after the lab (`cyberctf-<id>`), and a player's own copy of the same
// lab has the same name, so a name says nothing about whose it is. The run's folder (`root`, the
// app's CYBERCTF_WDIO_ROOT, made fresh by the runner) holds:
//
//   app/       the app's data (its labs in app/labs/<id>, its downloads record, the Isothrum
//              runner's record of each lab's run in app/runs/cyberctf-<id>), config, caches,
//              logs, and on Linux and Windows its webviews' data
//   home/      the app's files in the home folder (the session, the server secrets, the shared folder)
//   records/   what was on the machine when the run started: images.txt (the image ids), boxes.txt
//              (`vagrant box list`), each only when the tool could say
//
// A Compose project is the run's when its lab is installed in app/labs and every Compose file its
// containers were started from lies in the run's folder. An image or a box is the run's when it
// wasn't on the machine at the start. The specs and both runners decide through here (the runners
// through the command line below), and `owned.test.ts` holds it.
//
//   node tests/desktop/lib/owned.mjs record <root>     what is on the machine now, into <root>/records
//   node tests/desktop/lib/owned.mjs projects <root>   a line per Compose project: `ours` or `theirs`, a tab, its name
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** A lab's Compose project (and run) name, as the app names it (`runtime::compose_project`). */
export const projectOf = (id) => `cyberctf-${id}`;

/** The run's folders under its `root`. */
export function runFolders(root) {
  return {
    root,
    app: join(root, "app"),
    labs: join(root, "app", "labs"),
    runs: join(root, "app", "runs"),
    home: join(root, "home"),
    records: join(root, "records"),
  };
}

/** The run's folder (CYBERCTF_WDIO_ROOT, which the runner gives the app and the specs), or null
 *  outside a run.
 *  @param {Record<string, string | undefined>} env */
export const runRoot = (env = process.env) => env.CYBERCTF_WDIO_ROOT || null;

/** Whether `path` is `root` or lies in it: either separator, and on Windows in any case. */
export function isUnder(path, root, platform = process.platform) {
  const norm = (p) => {
    const s = String(p)
      .trim()
      .replace(/[\\/]+/g, "/")
      .replace(/\/$/, "");
    return platform === "win32" ? s.toLowerCase() : s;
  };
  const [p, r] = [norm(path), norm(root)];
  return r !== "" && (p === r || p.startsWith(`${r}/`));
}

/** `docker compose ls -a --format json`, read: each project's name and the Compose files its
 *  containers were started from. */
export function parseComposeLs(json) {
  let list;
  try {
    list = JSON.parse(json || "[]");
  } catch {
    return [];
  }
  return (Array.isArray(list) ? list : []).map((p) => ({
    name: String(p.Name ?? ""),
    configFiles: String(p.ConfigFiles ?? "")
      .split(",")
      .map((f) => f.trim())
      .filter(Boolean),
  }));
}

/** Which of `projects` the run started, given the labs installed in its folder (`labIds`) and its
 *  `root`: `ours` a project named after one of them whose every Compose file is in the run's
 *  folder; `theirs` every other one, a project of the same name started from elsewhere included. */
export function ownedProjects(projects, { root, labIds }, platform = process.platform) {
  const names = new Set(labIds.map(projectOf));
  const ours = [];
  const theirs = [];
  for (const p of projects) {
    const mine = names.has(p.name) && p.configFiles.length > 0 && p.configFiles.every((f) => isUnder(f, root, platform));
    (mine ? ours : theirs).push(p.name);
  }
  return { ours, theirs };
}

/** `docker image ls -q --no-trunc`, read: the image ids. */
export const parseImageIds = (text) =>
  new Set(
    String(text)
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter(Boolean),
  );

/** `vagrant box list`, read: each box version as the app lists it, `name (provider, version)`
 *  (an architecture Vagrant adds after the version is left out). */
export function parseBoxList(text) {
  const boxes = new Set();
  for (const line of String(text).split(/\r?\n/)) {
    const m = line.trim().match(/^(\S+)\s+\(([^,()]+),\s*([^,()]+)/);
    if (m) boxes.add(boxLabel(m[1], m[2].trim(), m[3].trim()));
  }
  return boxes;
}

/** A box version as the app's downloads list names it (`machine::storage`'s `box_label`). */
export const boxLabel = (name, provider, version) => `${name} (${provider}, ${version})`;

/** What the run recorded at its start, from `<root>/records`: the image ids and the box versions,
 *  each null when it wasn't recorded (its tool couldn't say). */
export function readBefore(root) {
  const read = (file, parse) => {
    const path = join(runFolders(root).records, file);
    return existsSync(path) ? parse(readFileSync(path, "utf8")) : null;
  };
  return { images: read("images.txt", parseImageIds), boxes: read("boxes.txt", parseBoxList) };
}

/**
 * Of what the app's Clean up would remove (`listed`: `machine_storage`'s answer, its images,
 * boxes and plugins), what the run itself downloaded: an image whose id (`imageId(reference)`,
 * now) wasn't on the machine at the start, a box version that wasn't, and the Terraform plugins
 * (in the app's data folder, the run's). Anything else is `theirs`, and so is everything of a
 * kind whose start wasn't recorded (`before.images` or `before.boxes` null).
 */
export function ownedDownloads(listed, before, imageId) {
  const ours = [];
  const theirs = [];
  for (const { name } of listed?.images ?? []) {
    const id = before.images ? imageId(name) : null;
    (id && !before.images.has(id) ? ours : theirs).push(name);
  }
  for (const { name } of listed?.boxes ?? []) (before.boxes && !before.boxes.has(name) ? ours : theirs).push(name);
  for (const { name } of listed?.plugins ?? []) ours.push(name);
  return { ours, theirs };
}

// --- The machine, through `run(program, args)`: its output, or null when it failed --------------

/** `program args`, without a shell; its output, null when it failed, "" when it isn't installed. */
export function runTool(program, args) {
  try {
    return execFileSync(program, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 120000, windowsHide: true });
  } catch (e) {
    return e?.code === "ENOENT" ? "" : null;
  }
}

/** The volumes `project`'s containers hold now (`docker ps -a` by the Compose project label, then
 *  the containers' volume mounts), each once: what a `down --volumes` leaves when the engine counts
 *  one named (an update's recreate inherits a service's anonymous volumes into the new container
 *  under their ids, and the engine then keeps them at the down). The runner records them the same
 *  way inside its operations; the harness's own down, outside the app, records them here. Empty
 *  when the engine doesn't answer. */
export function projectVolumes(project, run = runTool) {
  const ids = (run("docker", ["ps", "-aq", "--filter", `label=com.docker.compose.project=${project}`]) ?? "").split(/\s+/).filter(Boolean);
  if (!ids.length) return [];
  const mounts = run("docker", ["inspect", "--format", '{{range .Mounts}}{{if eq .Type "volume"}}{{.Name}}{{"\\n"}}{{end}}{{end}}', ...ids]);
  return volumeNames(mounts ?? "");
}

/** The volume names in `docker inspect`'s mount listing, each once, in order. */
export function volumeNames(listing) {
  return [...new Set(listing.split(/\s+/).filter(Boolean))];
}

/** Removes, among `names`, the volumes no container holds any more (the engine's dangling ones):
 *  the ones a `down --volumes` left. The names removed, in order. */
export function removeDangling(names, run = runTool) {
  if (!names.length) return [];
  const dangling = new Set((run("docker", ["volume", "ls", "--quiet", "--filter", "dangling=true"]) ?? "").split(/\s+/).filter(Boolean));
  const left = names.filter((n) => dangling.has(n));
  if (left.length) run("docker", ["volume", "rm", "--force", ...left]);
  return left;
}

/** The labs installed in the run's folder. */
export function installedLabs(root) {
  const labs = runFolders(root).labs;
  if (!existsSync(labs)) return [];
  return readdirSync(labs, { withFileTypes: true })
    .filter((e) => e.isDirectory() && !e.name.startsWith("."))
    .map((e) => e.name);
}

/** The Compose projects here, the run's and the others (see `ownedProjects`). */
export function projects(root, run = runTool, platform = process.platform) {
  const out = run("docker", ["compose", "ls", "-a", "--format", "json"]);
  return ownedProjects(parseComposeLs(out ?? ""), { root, labIds: installedLabs(root) }, platform);
}

/** The containers of the run's Compose projects, by name. */
export function runContainers(root, run = runTool, platform = process.platform) {
  return projects(root, run, platform).ours.flatMap((p) =>
    (run("docker", ["ps", "-a", "--format", "{{.Names}}", "--filter", `label=com.docker.compose.project=${p}`]) ?? "")
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter(Boolean),
  );
}

/**
 * The app's record of run `name` (`<root>/app/runs/<name>`, the Isothrum runner's) dropped, after
 * the run's clean-up took that Compose project down behind the app's back: the record still says
 * the lab runs, and the app reads it, from its files alone, before replacing the lab's folder, so
 * the next spec's seed (an import again, then a start) would be refused ("This lab is still
 * running on this machine"). Only a lab's run (`cyberctf-<id>`) of a lab installed in the run's
 * folder, a Compose one on this machine (driver `compose`, target `docker`, no host), whose record reads;
 * anything else (a VM's run, which still holds its machines) stays. Whether it was dropped.
 */
export function dropComposeRecord(root, name) {
  const { labs, runs } = runFolders(root);
  if (!/^cyberctf-[a-z0-9][a-z0-9-]*$/.test(name) || !existsSync(join(labs, name.slice("cyberctf-".length)))) return false;
  const dir = join(runs, name);
  let run;
  try {
    run = JSON.parse(readFileSync(join(dir, "record.json"), "utf8"));
  } catch {
    return false;
  }
  if (run?.name !== name || run.driver !== "compose" || run.target !== "docker" || run.host_label) return false;
  rmSync(dir, { recursive: true, force: true });
  return !existsSync(dir);
}

/** Whether a Vagrant machine was created in the run's folder (a VM lab or Docker in a VM of its). */
export function runVagrantMachines(root) {
  const found = [];
  const walk = (dir, depth) => {
    if (depth > 6 || !existsSync(dir)) return;
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (!e.isDirectory()) continue;
      const path = join(dir, e.name);
      if (e.name === ".vagrant") {
        const machines = join(path, "machines");
        if (existsSync(machines)) for (const m of readdirSync(machines)) found.push(join(machines, m));
      } else walk(path, depth + 1);
    }
  };
  walk(runFolders(root).labs, 0);
  return found;
}

/**
 * Whether a lab a page shows running before a spec started it is the run's to stop (an earlier
 * spec's): only when no lab the run didn't start runs here (a Compose project `cyberctf-*` that
 * isn't the run's: the player's own copy, which the app's running-labs scan shows too) and the
 * run started one (a Compose project, or a Vagrant machine in its folder). `why` says otherwise.
 */
export function leftoverIsTheRuns(root, run = runTool, platform = process.platform) {
  if (!root) return { ok: false, why: "no run folder (CYBERCTF_WDIO_ROOT): run the spec through tests/desktop/run.sh or run.ps1" };
  const { ours, theirs } = projects(root, run, platform);
  const others = theirs.filter((p) => p.startsWith("cyberctf-"));
  if (others.length) return { ok: false, why: `labs this run didn't start run here (${others.join(", ")}), left as they are` };
  if (ours.length || runVagrantMachines(root).length) return { ok: true, why: "" };
  return { ok: false, why: "the lab runs, but not from this run: left as it is" };
}

/** Writes what is on the machine now into `<root>/records`: each kind only when its tool could
 *  say (none installed: nothing of it here). */
export function record(root, run = runTool) {
  const dir = runFolders(root).records;
  mkdirSync(dir, { recursive: true });
  const images = run("docker", ["image", "ls", "-q", "--no-trunc"]);
  if (images !== null) writeFileSync(join(dir, "images.txt"), images);
  const boxes = run("vagrant", ["box", "list"]);
  if (boxes !== null) writeFileSync(join(dir, "boxes.txt"), boxes);
}

// Run as a command (not imported): the same file, either separator, any case on Windows.
if (process.argv[1] && isUnder(fileURLToPath(import.meta.url), process.argv[1])) {
  const [what, root] = process.argv.slice(2);
  if (!root) {
    console.error("usage: owned.mjs record|projects <run folder>");
    process.exit(2);
  }
  if (what === "record") record(root);
  else if (what === "projects") {
    const { ours, theirs } = projects(root);
    for (const p of ours) console.log(`ours\t${p}`);
    for (const p of theirs) console.log(`theirs\t${p}`);
  } else {
    console.error(`owned.mjs: unknown command ${what}`);
    process.exit(2);
  }
}
