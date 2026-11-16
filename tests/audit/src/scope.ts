// What of the new things on a machine a run left: the allowlist (what a run may leave, each with
// its reason) and the run's own (its scope). On a throwaway clone everything new is the run's; on a
// machine someone uses (the owner's Mac) a thing is the run's only when the run proves it made it:
// it lies under one of the run's folders, or its name is one the run used (given, or in the run's
// logs) and it was made while the run (or the pass whose logs name it) ran. Never a blanket diff.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import type { Item } from "./inventory.ts";

/** Where a run ran: a throwaway `clone`, or a `shared` machine someone else uses too. */
export type Mode = "clone" | "shared";

/** A stretch of time the run made things in (epoch seconds), with the text of the logs that
 *  name what it made then, when they're given apart from the run's. */
export type Window = { start: number; end: number; text?: string };

/** The run's scope: its mode, its folders, the names it used, the text of its logs, its windows. */
export type Scope = { mode: Mode; roots: string[]; names: string[]; logText: string; windows: Window[] };

/** What a run may leave: the kind, where (`clone`, `shared` or `any`), a glob on the name, why. A
 *  glob that starts with `!` says what it may not leave, even when a later entry would allow it:
 *  the first entry that matches decides. */
export type Allowance = { kind: string; where: Mode | "any"; pattern: string; reason: string };

/** A glob (`*` any run of characters, `?` one, `[0-9]` one of a class) as a whole-name regular
 *  expression. */
export function globRegExp(glob: string): RegExp {
  const body = glob
    .replace(/[.+^${}()|\\]/g, "\\$&")
    .replaceAll("*", ".*")
    .replaceAll("?", ".");
  return new RegExp(`^${body}$`, "s");
}

/** An allowlist's entries: TSV lines `kind<TAB>where<TAB>glob<TAB>reason`, `#` comments. Each
 *  must say why: an entry without a reason is an error. */
export function readAllowlist(text: string, source: string): Allowance[] {
  return text.split("\n").flatMap((raw, i) => {
    const line = raw.replace(/\r$/, "");
    if (line.trim() === "" || line.startsWith("#")) return [];
    const [kind, where, pattern, reason] = line.split("\t");
    if (!reason?.trim() || !["clone", "shared", "any"].includes(where)) {
      throw new Error(`${source}:${String(i + 1)}: an allowance is kind<TAB>clone|shared|any<TAB>glob<TAB>reason, its reason said`);
    }
    return [{ kind, where: where as Allowance["where"], pattern, reason: reason.trim() }];
  });
}

/** The allowance that lets the run leave `item` here, if any: the first entry that matches it, unless
 *  that one is a `!` entry. */
export function allowanceFor(item: Item, mode: Mode, allowlist: readonly Allowance[]): Allowance | undefined {
  const entry = allowlist.find(
    (a) => a.kind === item.kind && (a.where === "any" || a.where === mode) && globRegExp(a.pattern.replace(/^!/, "")).test(item.name),
  );
  return entry?.pattern.startsWith("!") ? undefined : entry;
}

/** Whether `path` is `root` or lies under it (either separator). */
export function under(path: string, root: string): boolean {
  const norm = (p: string) => p.replaceAll("\\", "/").replace(/\/+$/, "");
  const p = norm(path);
  const r = norm(root);
  return r !== "" && (p === r || p.startsWith(`${r}/`));
}

/** The folders that lie in a copy of a project: Isoloom's outputs, Vagrant's state. */
const PROJECT_MARKERS = new Set([".isoloom", ".vagrant"]);

/** The project folder `path` lies in, which tells one copy of a lab from another: the folder its
 *  `.isoloom` (or Vagrant's `.vagrant`) lies in, so `…/labs/pm-9221/.isoloom/vagrant` is pm-9221's
 *  and `…/T/cyberctf-dvm-21666/.isoloom/docker-vm` cyberctf-dvm-21666's. None for a path outside
 *  one. */
export function projectOf(path: string): string | undefined {
  const parts = path.split(/[\\/]/).filter((p) => p !== "");
  const i = parts.findIndex((p) => PROJECT_MARKERS.has(p));
  return i > 0 ? parts[i - 1] : undefined;
}

/** Whether `text` names `ref` as a whole: not as a piece of a longer word or name ("data" in
 *  "database", "pm-9221" in "pm-92210"). */
export function mentions(text: string, ref: string): boolean {
  const escaped = ref.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(?<![\\w])${escaped}(?![\\w])`).test(text);
}

/** The names that make `item` the run's when the run names one. A thing in a copy of a project (a
 *  Vagrant machine, a container or process of a lab's folder) by its project folder alone: copies
 *  run side by side share their machines' names ("database", "default"), and those turn up in any
 *  log (apt's "Reading database …"). Anything else by its names. Four characters at least. */
function namesOf(item: Item): string[] {
  const projects = [...new Set(item.paths.map(projectOf).filter((p): p is string => p !== undefined))];
  return (projects.length > 0 ? projects : item.refs).filter((r) => r.length >= 4);
}

/** Whether the run made `item`: on a clone, always; on a shared machine, when it lies under one of
 *  the run's folders, when it is a socket of a process the run made (`pids`), or when the run's
 *  names or logs name it (`namesOf`, each as a whole) and it was made in one of the run's
 *  windows (its date unknown: the name decides). A window with logs of its own needs them to name
 *  it. */
export function ownedByRun(item: Item, scope: Scope, pids: ReadonlySet<number>): boolean {
  if (scope.mode === "clone") return true;
  if (item.paths.some((p) => scope.roots.some((r) => under(p, r)))) return true;
  if (item.kind === "listening-port") return item.pid !== undefined && pids.has(item.pid);
  const names = namesOf(item);
  const named = (text: string) => names.some((r) => scope.names.includes(r) || mentions(text, r));
  if (!named(scope.logText) && !scope.windows.some((w) => w.text !== undefined && named(w.text))) return false;
  if (scope.windows.length === 0 || item.created === undefined) return true;
  const { created } = item;
  // A minute's grace after a window: a tool's clock, a pass's last cleanup.
  return scope.windows.some((w) => created >= w.start && created <= w.end + 60 && (w.text === undefined || named(w.text)));
}

/** The files the leftover audit itself writes (its snapshots, records and report), never read as
 *  the run's logs: they name everything. */
const AUDIT_FILE = /^audit[-.]/;

/** The text of the logs at `paths` (files, or folders read whole), the audit's own files and
 *  anything over 50 MB left out. */
export function readLogs(paths: readonly string[]): string {
  const parts: string[] = [];
  const visit = (path: string) => {
    if (!existsSync(path)) return;
    const st = statSync(path);
    if (st.isDirectory()) {
      for (const entry of readdirSync(path)) if (!AUDIT_FILE.test(entry)) visit(join(path, entry));
    } else if (st.isFile() && st.size < 50_000_000) {
      parts.push(readFileSync(path, "latin1"));
    }
  };
  paths.forEach(visit);
  return parts.join("\n");
}

/** A `--window` argument: `<start>:<end>`, epoch seconds, then `=<log>[,<log>…]` for the logs of
 *  that window alone (a pass's). */
export function parseWindow(arg: string): Window {
  const m = /^(\d+):(\d+)(?:=(.+))?$/.exec(arg);
  if (!m) throw new Error(`--window takes <start>:<end>[=<log>,<log>…] in epoch seconds, not ${arg}`);
  const window: Window = { start: Number(m[1]), end: Number(m[2]) };
  if (m[3]) window.text = readLogs(m[3].split(","));
  return window;
}
