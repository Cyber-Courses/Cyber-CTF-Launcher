// Clippy over the crate and its tests, read as JSON: every warning is a finding (counted by the
// ratchet), every error fails the run. `cargo machete` must find no unused dependency.

import { spawnSync } from "node:child_process";
import path from "node:path";

import { type Finding, repoRelative } from "./model.ts";

type Span = { file_name: string; line_start: number; column_start: number; is_primary: boolean; expansion?: { span: Span } | null };
type Diagnostic = { level: string; message: string; rendered: string | null; code: { code: string } | null; spans: Span[] };
type CargoMessage = { reason: string; message?: Diagnostic };

/** The crate, relative to the repository root. */
const CRATE = "src-tauri";

/** The span in the crate's own sources a diagnostic points at, through macro expansions. */
function ownSpan(spans: readonly Span[]): Span | undefined {
  let span = spans.find((s) => s.is_primary) ?? spans.at(0);
  while (span && path.isAbsolute(span.file_name) && span.expansion) span = span.expansion.span;
  return span;
}

/** A warning's finding, keyed by its place; null for another level, a lint-less message, or a
 *  file outside the repository: a path dependency there (Isoloom through `cargo xtask dev-link`)
 *  has its own ratchet. */
function findingOf(root: string, msg: Diagnostic): { key: string; finding: Finding } | null {
  const span = ownSpan(msg.spans);
  if (msg.level !== "warning" || !msg.code || !span) return null;
  // Relative to the crate for its own files, with the platform's separator (`src\app.rs` on
  // Windows, which used to count every Windows finding in `rs:root`).
  const file = repoRelative(root, path.resolve(root, CRATE, span.file_name));
  if (file.startsWith("..")) return null;
  const finding = {
    rule: msg.code.code.replace(/^clippy::/, "clippy/").replace(/^(?!clippy\/)/, "rustc/"),
    file,
    line: span.line_start,
    message: msg.message,
  };
  return { key: `${finding.rule} ${file}:${String(span.line_start)}:${String(span.column_start)}`, finding };
}

/** The clippy and rustc warnings in cargo's JSON output `stdout`, deduplicated by place (the lib
 *  is checked as itself and as its tests, where a future's size, say, can differ), and the rendered
 *  errors; a run that exited with `status` other than 0 and no error is one too. */
export function clippyOutcome(root: string, stdout: string, status: number | null): { warnings: Finding[]; errors: string[] } {
  const warnings = new Map<string, Finding>();
  const errors: string[] = [];
  for (const line of stdout.split("\n")) {
    if (!line.startsWith("{")) continue;
    const msg = (JSON.parse(line) as CargoMessage).message;
    if (!msg) continue;
    if (msg.level === "error") errors.push(msg.rendered ?? msg.message);
    const found = findingOf(root, msg);
    if (found) warnings.set(found.key, found.finding);
  }
  if (status !== 0 && errors.length === 0) errors.push(`cargo clippy exited with ${String(status)}`);
  return { warnings: [...warnings.values()], errors };
}

/** Clippy over the crate and its tests: its warnings and errors (`clippyOutcome`). */
export function runClippy(root: string): { warnings: Finding[]; errors: string[] } {
  const args = ["clippy", "--manifest-path", path.join(CRATE, "Cargo.toml"), "--all-targets", "--message-format=json"];
  const run = spawnSync("cargo", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "inherit"] });
  if (run.error) throw run.error;
  return clippyOutcome(root, run.stdout, run.status);
}

/** What a `cargo machete` run says: why it couldn't run, the unused dependencies it found, or null
 *  when every dependency is used. */
export function macheteComplaint(run: { error?: Error; status: number | null; stdout: string; stderr: string }): string | null {
  if (run.error) return `cargo machete couldn't run (cargo install cargo-machete): ${run.error.message}`;
  return run.status === 0 ? null : run.stdout + run.stderr;
}

/** `cargo machete`'s complaint over the crate (`macheteComplaint`). */
export function runMachete(root: string): string | null {
  return macheteComplaint(spawnSync("cargo", ["machete", "--skip-target-dir", CRATE], { cwd: root, encoding: "utf8" }));
}
