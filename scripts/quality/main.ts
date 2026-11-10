// The quality ratchet (`pnpm quality`): clippy, ESLint, the source scans, jscpd and knip, counted
// per rule and per area and held to quality-baseline.json. Any error fails; a count above the
// baseline fails; a count below it is reported, to lock in with `--update`.
//
//   pnpm quality                 check
//   pnpm quality --update        lower the baseline (refused if a rule's total rose)
//   pnpm quality --update --force   accept new counts too (a new rule; reviewed in the diff)
//   pnpm quality --list [prefix] print every finding, by area (e.g. --list rs:machine)
//   pnpm quality --help          this text

import path from "node:path";
import { fileURLToPath } from "node:url";

import { helpAsked } from "../lib/cli.ts";
import { type Baseline, ratchet, readBaseline, writeBaseline } from "./baseline.ts";
import { runClippy, runMachete } from "./clippy.ts";
import { eslintExceptions, lintedFiles, runEslint } from "./eslint.ts";
import { type Finding, type Measure, areaOf, countExceptions, countFindings, sorted, sortedKeys, total } from "./model.ts";
import { runJscpd, runKnip } from "./node-tools.ts";
import { rustSources, scanRust } from "./rust-scan.ts";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const BASELINE = path.join(ROOT, "quality-baseline.json");

/** Runs every check; the errors that fail the run whatever the counts. */
async function measure(): Promise<{ measured: Measure; errors: string[] }> {
  const errors: string[] = [];
  const clippy = runClippy(ROOT);
  errors.push(...clippy.errors);
  const machete = runMachete(ROOT);
  if (machete) errors.push(machete);
  const eslint = await runEslint(ROOT);
  if (eslint.errors) errors.push(eslint.errors);
  const rust = scanRust(rustSources(ROOT, "src-tauri/src"));
  const exceptions = [...rust.exceptions, ...eslintExceptions(ROOT, lintedFiles(ROOT))];
  const unexplained = exceptions.filter((e) => !e.hasReason).map((e) => ({ rule: "exception/without-reason", file: e.file, line: 1, message: e.text }));
  const findings = [...rust.findings, ...eslint.warnings, ...runJscpd(ROOT), ...runKnip(ROOT), ...unexplained];
  return { measured: { findings, clippy: clippy.warnings, exceptions }, errors };
}

function toBaseline(m: Measure, previous: Baseline): Baseline {
  const clippy = { ...previous.clippy, [process.platform]: sorted(countFindings(m.clippy)) };
  return { rules: sorted(countFindings(m.findings)), clippy: sortedKeys(clippy), exceptions: sorted(countExceptions(m.exceptions)) };
}

/** Each rule's total, and how much of it is outside `runtime/` (which moves to the runner). */
function summary(b: Baseline): string {
  const rows = [...Object.entries(b.rules), ...Object.entries(b.clippy[process.platform] ?? {})].sort(([a], [z]) => a.localeCompare(z));
  return rows
    .map(([rule, byArea]) => {
      const runtime = byArea?.["rs:runtime"] ?? 0;
      return `  ${rule.padEnd(52)} ${String(total(byArea) - runtime).padStart(5)}${runtime ? `  (+${String(runtime)} in runtime/)` : ""}`;
    })
    .join("\n");
}

function list(m: Measure, prefix: string): void {
  const all: Finding[] = [...m.findings, ...m.clippy].filter((f) => areaOf(f.file).startsWith(prefix));
  all.sort((a, b) => areaOf(a.file).localeCompare(areaOf(b.file)) || a.file.localeCompare(b.file) || a.line - b.line);
  for (const f of all) console.log(`${areaOf(f.file)}\t${f.rule}\t${f.file}:${String(f.line)}\t${f.message}`);
}

async function main(args: readonly string[]): Promise<number> {
  const { measured, errors } = await measure();
  if (args[0] === "--list") list(measured, args[1] ?? "");
  const before = readBaseline(BASELINE);
  const now = toBaseline(measured, before);
  console.log(`Violations outside runtime/ (${process.platform}):\n${summary(now)}`);
  const { write, failures, notes } = ratchet(now, before, process.platform, { update: args.includes("--update"), force: args.includes("--force") });
  for (const line of [...errors, ...failures]) console.error(line);
  for (const line of notes) console.log(line);
  if (write) writeBaseline(BASELINE, now);
  return errors.length > 0 || failures.length > 0 ? 1 : 0;
}

if (!helpAsked(import.meta.url, process.argv.slice(2))) process.exitCode = await main(process.argv.slice(2));
