// The feature registry's check and generator. tests/features.yml lists every feature the launcher
// offers its user and the tests that check it, tier by tier; this holds it to the repository and
// writes tests/FEATURES.md from it (README.md in tests/, "The feature registry").
//
//   pnpm check:features                  check: every test named exists, every command, screen,
//                                        integration scenario and desktop case belongs to a feature,
//                                        and FEATURES.md is what the registry makes
//   pnpm features                        write FEATURES.md (after the same check)
//   pnpm features --matrix <MATRIX.md>   first refresh tests/features-rig.json from the test rig's
//                                        matrix (the workspace's rig/MATRIX.md), then write
//   pnpm check:features --help           this text
//
// The check also compares the rig snapshot with the workspace's matrix when the launcher is the
// workspace's submodule (../rig/MATRIX.md) or CYBERCTF_TEST_RIG_MATRIX names one: a newer matrix
// is a note, not a failure (the workspace moves on its own).
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as prettier from "prettier";

import { helpAsked } from "../lib/cli.ts";
import { problems } from "./check.ts";
import { levelCounts } from "./coverage.ts";
import { readFacts } from "./facts.ts";
import { parseMatrix, RIG_COLUMNS, type RigSnapshot } from "./matrix.ts";
import { features, parseRegistry } from "./registry.ts";
import { render } from "./render.ts";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const REGISTRY = "tests/features.yml";
const OUTPUT = "tests/FEATURES.md";
const RIG = "tests/features-rig.json";

type Args = { write: boolean; matrix: string | null };

/** Reads the arguments: `--write`, `--matrix <file>` (which writes too). Anything else is refused. */
export function parseArgs(args: readonly string[]): Args | { error: string } {
  const out: Args = { write: false, matrix: null };
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "--write") out.write = true;
    else if (a === "--matrix" && args[i + 1] && !args[i + 1].startsWith("--")) {
      out.matrix = args[++i];
      out.write = true;
    } else return { error: `unknown argument \`${a}\` (see pnpm check:features --help)` };
  }
  return out;
}

/** The markdown `render` made, formatted as Prettier formats the repository's Markdown. */
export async function formatted(markdown: string, file: string): Promise<string> {
  const options = (await prettier.resolveConfig(file)) ?? {};
  return prettier.format(markdown, { ...options, parser: "markdown", filepath: file });
}

function readRig(): RigSnapshot {
  const file = path.join(ROOT, RIG);
  if (!existsSync(file)) return { generated: "", columns: RIG_COLUMNS.map(([id]) => id), rows: [] };
  return JSON.parse(readFileSync(file, "utf8")) as RigSnapshot;
}

/** The workspace's matrix, when there is one to compare with. */
function workspaceMatrix(): string | null {
  const named = process.env.CYBERCTF_TEST_RIG_MATRIX;
  if (named) return named;
  const sibling = path.join(ROOT, "..", "rig", "MATRIX.md");
  return existsSync(sibling) ? sibling : null;
}

/** The snapshot as its file holds it: a row a line, so a rig run's diff reads row by row. */
export function snapshotJson(s: RigSnapshot): string {
  const rows = s.rows.map((r) => `    ${JSON.stringify(r)}`).join(",\n");
  return `{\n  "generated": ${JSON.stringify(s.generated)},\n  "columns": ${JSON.stringify(s.columns)},\n  "rows": [\n${rows}\n  ]\n}\n`;
}

function refreshRig(matrix: string): RigSnapshot {
  const snapshot = parseMatrix(readFileSync(matrix, "utf8"));
  writeFileSync(path.join(ROOT, RIG), snapshotJson(snapshot));
  console.log(`${RIG}: the rig's matrix of ${snapshot.generated}`);
  return snapshot;
}

function rigNote(rig: RigSnapshot): void {
  const matrix = workspaceMatrix();
  if (!matrix || !existsSync(matrix)) return;
  const latest = parseMatrix(readFileSync(matrix, "utf8"));
  if (latest.generated !== rig.generated)
    console.log(`note: ${matrix} is of ${latest.generated}, ${RIG} of ${rig.generated}: pnpm features --matrix ${matrix}`);
}

async function main(argv: readonly string[]): Promise<number> {
  const args = parseArgs(argv);
  if ("error" in args) {
    console.error(args.error);
    return 2;
  }
  const rig = args.matrix ? refreshRig(args.matrix) : readRig();
  const { registry, problems: shape } = parseRegistry(readFileSync(path.join(ROOT, REGISTRY), "utf8"));
  const found = [...shape, ...problems(registry, readFacts(ROOT), rig)];
  const output = path.join(ROOT, OUTPUT);
  const made = await formatted(render(registry, rig), output);
  if (args.write) writeFileSync(output, made);
  else if (!existsSync(output) || readFileSync(output, "utf8") !== made) found.push(`${OUTPUT} is stale: pnpm features`);
  for (const p of found) console.error(p);
  const n = levelCounts(features(registry), rig);
  console.log(
    `${String(features(registry).length)} features in ${String(registry.areas.length)} areas: ${String(n.Strong)} Strong, ${String(n.Partial)} Partial, ${String(n.Unchecked)} Unchecked; ${String(found.length)} problems.`,
  );
  if (!args.matrix) rigNote(rig);
  return found.length ? 1 : 0;
}

if (process.argv[1]?.endsWith("main.ts") && !helpAsked(import.meta.url, process.argv.slice(2))) process.exit(await main(process.argv.slice(2)));
