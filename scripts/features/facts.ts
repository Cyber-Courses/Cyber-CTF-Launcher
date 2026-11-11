// What the repository holds that the feature registry names: the Tauri commands, the screens and
// windows, the integration scenarios and desktop cases (from the runners' `--list`), the desktop
// specs' check ids, the guided setup's items, the test files. Each reader takes a file's text, so
// the unit tests hand it theirs; `readFacts` reads the repository's.
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";

export type Facts = {
  /** The commands the webview may invoke (`generate_handler!` in src-tauri/src/commands.rs). */
  commands: string[];
  /** Each screen and window: `route:/<path>` (src/app), `tab:<id>` (the shell's screens),
   *  `settings:<id>` (the Settings window's sections). */
  pages: string[];
  /** The integration scenarios, as `tests/integration/run.sh --list` names them. */
  scenarios: string[];
  /** The desktop cases (`tests/desktop/run.sh --list`), each with its spec. */
  desktopCases: Map<string, string>;
  /** Each desktop spec's check ids. */
  desktopChecks: Map<string, Set<string>>;
  /** The guided setup's items (integration_tests::setup), `<x>` standing for any name. */
  setupItems: string[];
  /** A repository file's text, or null when there is none. */
  read: (file: string) => string | null;
};

/** The command names registered in commands.rs's text (each path's last segment). */
export function commandsOf(source: string): string[] {
  const body = /generate_handler!\[([\s\S]*?)\]\)/.exec(source)?.[1] ?? "";
  return body
    .split(",")
    .map((n) => n.trim().split("::").at(-1) ?? "")
    .filter(Boolean);
}

/** The route of each `page.tsx` under src/app (paths relative to src/app, `/` separated). */
export const routesOf = (pageFiles: string[]): string[] =>
  pageFiles.map((f) => `route:/${path.posix.dirname(f.replaceAll("\\", "/")).replace(/^\.$/, "")}`).sort();

/** The shell's screens: the members of `export type Tab = "home" | …` in shell-model.ts. */
export function tabsOf(source: string): string[] {
  const union = /export type Tab\s*=([^;]*);/.exec(source)?.[1] ?? "";
  return [...union.matchAll(/"([^"]+)"/g)].map((m) => `tab:${m[1]}`);
}

/** The Settings window's sections: `{ id: "…", label: "settings.tabs.…"` in settings-screen.tsx. */
export const settingsSectionsOf = (source: string): string[] =>
  [...source.matchAll(/\{\s*id:\s*"([^"]+)",\s*label:\s*"settings\.tabs\./g)].map((m) => `settings:${m[1]}`);

/** The `case` of each JSON line a runner's `--list` printed (other lines ignored). */
export function listedCases(output: string): { case: string; spec?: string }[] {
  return output
    .split("\n")
    .filter((l) => l.trim().startsWith("{"))
    .map((l) => JSON.parse(l) as { case: string; spec?: string });
}

/** A desktop spec's check ids: the first argument of `r.check`, `r.step`, `r.pass`, `r.fail`,
 *  `r.skip`, plus every string literal a `r.skip(id…)` loop could take, so a check id read from a
 *  list still counts. */
export function checkIdsOf(source: string): Set<string> {
  const ids = new Set<string>();
  for (const m of source.matchAll(/\br\.(?:check|step|pass|fail|skip)\(\s*"([^"]+)"/g)) ids.add(m[1]);
  if (/\br\.skip\(\s*[a-zA-Z_]/.test(source)) for (const m of source.matchAll(/"([a-z][a-z0-9-]*)"/g)) ids.add(m[1]);
  return ids;
}

/** The setup's items, as its module documentation lists them ("The items: `wsl`, `docker`, …."). */
export function setupItemsOf(source: string): string[] {
  const doc = source
    .split("\n")
    .filter((l) => l.startsWith("//!"))
    .map((l) => l.replace(/^\/\/! ?/, ""))
    .join(" ");
  const list = /The items:(.*?)\.(?:\s|$)/.exec(doc)?.[1] ?? "";
  return [...list.matchAll(/`([^`]+)`/g)].map((m) => m[1]);
}

/** Whether a setup reference matches an item: exactly, or an item's `<x>` part taking any name. */
export function matchesSetupItem(ref: string, items: string[]): boolean {
  return items.some(
    (item) =>
      item === ref ||
      new RegExp(
        `^${item
          .split(/<[^>]+>/)
          .map(escape)
          .join(".+")}$`,
      ).test(ref),
  );
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Every `page.tsx` under `dir`, relative to it. */
function pageFiles(dir: string, rel = ""): string[] {
  return readdirSync(path.join(dir, rel), { withFileTypes: true }).flatMap((e) => {
    const p = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) return pageFiles(dir, p);
    return e.name === "page.tsx" ? [p] : [];
  });
}

/** What `bash <runner> --list` printed; throws with its output when it failed. */
function list(root: string, runner: string): string {
  const r = spawnSync("bash", [path.join(root, runner), "--list"], { cwd: root, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${runner} --list failed (${String(r.status)}): ${r.stderr || r.stdout}`);
  return r.stdout;
}

/** The facts of the repository at `root`. */
export function readFacts(root: string): Facts {
  const read = (file: string) => {
    const p = path.join(root, file);
    return existsSync(p) ? readFileSync(p, "utf8") : null;
  };
  const must = (file: string) => {
    const t = read(file);
    if (t === null) throw new Error(`${file} is gone: scripts/features/facts.ts reads it`);
    return t;
  };
  const desktop = listedCases(list(root, "tests/desktop/run.sh"));
  const specs = [...new Set(desktop.map((c) => c.spec ?? c.case))];
  return {
    commands: commandsOf(must("src-tauri/src/commands.rs")),
    pages: [
      ...routesOf(pageFiles(path.join(root, "src/app"))),
      ...tabsOf(must("src/components/shell/shell-model.ts")),
      ...settingsSectionsOf(must("src/features/settings/settings-screen.tsx")),
    ],
    scenarios: listedCases(list(root, "tests/integration/run.sh")).map((c) => c.case),
    desktopCases: new Map(desktop.map((c) => [c.case, c.spec ?? c.case])),
    desktopChecks: new Map(specs.map((s) => [s, checkIdsOf(read(`tests/desktop/specs/${s}.mjs`) ?? "")])),
    setupItems: setupItemsOf(must("src-tauri/src/integration_tests/setup.rs")),
    read,
  };
}
