// The leftover audit's commands (tests/audit/run.sh runs them; tests/leftovers.md is the contract):
//
//   pnpm leftovers diff <before.json> <after.json> [options]   what the run left, kind by kind
//   pnpm leftovers inventory <snapshot.json>                   a snapshot's items, one JSON line each
//   pnpm leftovers --list                                      the kinds, one JSON line each
//
// diff's options:
//   --mode clone|shared   clone (default): a throwaway machine, everything new is the run's;
//                         shared: a machine someone uses, only what the run proves it made
//   --root <dir>          a folder of the run's own: what lies under it is the run's
//   --name <name>         a name the run used (a Compose project, a lab id)
//   --named-in <path>     the run's logs (a file or a folder): a name they hold is one it used
//   --window <s>:<e>[=<log>,…]  when the run made things (epoch seconds); with logs, those of
//                         that stretch alone must name a thing made in it
//   --allow <file>        more allowances (TSV, as tests/audit/allowlist.tsv)
//   --results <file>      a record per kind, appended (tests/README.md, "Results"), suite `audit`
//   --report <file>       the report for a person (default: standard output)
//
// Exit status: 0 nothing left, 1 the run left something, 2 the audit couldn't run.

import { appendFileSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { helpAsked } from "../../scripts/lib/cli.ts";
import { diff, report, toRecord } from "./diff.ts";
import { parseProbe, readSnapshot } from "./inventory.ts";
import { KINDS } from "./kinds.ts";
import { type Mode, type Scope, type Window, parseWindow, readAllowlist, readLogs } from "./scope.ts";

/** The allowlist that comes with the audit. */
export const ALLOWLIST = join(dirname(fileURLToPath(import.meta.url)), "allowlist.tsv");

type DiffArgs = { before: string; after: string; scope: Scope; allow: string[]; results?: string; reportFile?: string };

const VALUED = new Set(["--mode", "--root", "--name", "--named-in", "--window", "--allow", "--results", "--report"]);

/** diff's arguments, or an error saying which is wrong. */
export function parseDiffArgs(args: readonly string[]): DiffArgs {
  const positional: string[] = [];
  const opts: Record<string, string[]> = {};
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (VALUED.has(arg)) {
      const value = args[++i] as string | undefined;
      if (value === undefined) throw new Error(`${arg} takes a value`);
      (opts[arg] ??= []).push(value);
    } else if (arg.startsWith("--")) throw new Error(`unknown option ${arg}`);
    else positional.push(arg);
  }
  if (positional.length !== 2) throw new Error("diff takes the snapshots before and after");
  const mode = (opts["--mode"]?.at(-1) ?? "clone") as Mode;
  if (mode !== "clone" && mode !== "shared") throw new Error(`--mode is clone or shared, not ${String(mode)}`);
  const windows: Window[] = (opts["--window"] ?? []).map(parseWindow);
  const scope: Scope = { mode, roots: opts["--root"] ?? [], names: opts["--name"] ?? [], logText: readLogs(opts["--named-in"] ?? []), windows };
  return {
    before: positional[0],
    after: positional[1],
    scope,
    allow: opts["--allow"] ?? [],
    results: opts["--results"]?.at(-1),
    reportFile: opts["--report"]?.at(-1),
  };
}

/** diff: the report and records; 1 when the run left something. */
export function runDiff(a: DiffArgs): number {
  const before = readSnapshot(readFileSync(a.before, "utf8"), a.before);
  const after = readSnapshot(readFileSync(a.after, "utf8"), a.after);
  // The snapshots' roots are the run's too (snapshot.sh's --root).
  const scope = { ...a.scope, roots: [...new Set([...a.scope.roots, ...before.roots, ...after.roots])] };
  const allowlist = [ALLOWLIST, ...a.allow].flatMap((f) => readAllowlist(readFileSync(f, "utf8"), f));
  const results = diff(before, after, scope, allowlist);
  const text = report(results, before, after, scope);
  if (a.reportFile) writeFileSync(a.reportFile, text);
  else process.stdout.write(text);
  if (a.results) {
    const log = a.reportFile ?? "";
    appendFileSync(a.results, results.map((r) => `${JSON.stringify(toRecord(r, log))}\n`).join(""));
  }
  return results.some((r) => r.result === "fail") ? 1 : 0;
}

function main(args: string[]): number {
  if (helpAsked(import.meta.url, args)) return 0;
  const [command, ...rest] = args;
  if (command === "--list") {
    for (const k of KINDS) console.log(JSON.stringify({ suite: "audit", case: k.kind, on: k.on, tool: k.tool, what: k.what }));
    return 0;
  }
  if (command === "inventory" && rest.length === 1) {
    const snap = readSnapshot(readFileSync(rest[0], "utf8"), rest[0]);
    for (const p of snap.probes) for (const item of parseProbe(p, snap.taken)) console.log(JSON.stringify(item));
    return 0;
  }
  if (command === "diff") return runDiff(parseDiffArgs(rest));
  throw new Error("a command is needed: diff, inventory or --list (--help says how)");
}

/** Whether this file is the one Node was asked to run (by any path to it: a link, macOS's /var). */
export function isMain(argv1: string | undefined, url: string): boolean {
  if (!argv1) return false;
  try {
    return realpathSync(argv1) === realpathSync(fileURLToPath(url));
  } catch {
    return false;
  }
}

if (isMain(process.argv[1], import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (e) {
    console.error(`leftovers: ${e instanceof Error ? e.message : String(e)}`);
    process.exitCode = 2;
  }
}
