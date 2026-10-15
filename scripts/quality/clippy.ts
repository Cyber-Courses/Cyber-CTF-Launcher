// Clippy over the crate and its tests, read as JSON: every warning is a finding (counted by the
// ratchet), every error fails the run. `cargo machete` must find no unused dependency.

import { spawnSync } from "node:child_process";
import path from "node:path";

import type { Finding } from "./model.ts";

type Span = { file_name: string; line_start: number; column_start: number; is_primary: boolean; expansion?: { span: Span } | null };
type Diagnostic = { level: string; message: string; rendered: string | null; code: { code: string } | null; spans: Span[] };
type CargoMessage = { reason: string; message?: Diagnostic };

/** The crate, relative to the repository root. */
const CRATE = "src-tauri";

/** The span in the crate's own sources a diagnostic points at, through macro expansions. */
function ownSpan(spans: readonly Span[]): Span | undefined {
  let span = spans.find((s) => s.is_primary) ?? spans[0];
  while (span && path.isAbsolute(span.file_name) && span.expansion) span = span.expansion.span;
  return span;
}

/** The clippy and rustc warnings, deduplicated by place (the lib is checked as itself and as its
 *  tests, where a future's size, say, can differ), and the rendered errors. */
export function runClippy(root: string): { warnings: Finding[]; errors: string[] } {
  const args = ["clippy", "--manifest-path", path.join(CRATE, "Cargo.toml"), "--all-targets", "--message-format=json"];
  const run = spawnSync("cargo", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "inherit"] });
  if (run.error) throw run.error;
  const warnings = new Map<string, Finding>();
  const errors: string[] = [];
  for (const line of run.stdout.split("\n")) {
    if (!line.startsWith("{")) continue;
    const msg = (JSON.parse(line) as CargoMessage).message;
    if (!msg) continue;
    if (msg.level === "error") errors.push(msg.rendered ?? msg.message);
    const span = ownSpan(msg.spans);
    if (msg.level !== "warning" || !msg.code || !span) continue;
    const file = path.isAbsolute(span.file_name) ? path.relative(root, span.file_name) : `${CRATE}/${span.file_name}`;
    const finding = {
      rule: msg.code.code.replace(/^clippy::/, "clippy/").replace(/^(?!clippy\/)/, "rustc/"),
      file,
      line: span.line_start,
      message: msg.message,
    };
    warnings.set(`${finding.rule} ${file}:${String(span.line_start)}:${String(span.column_start)}`, finding);
  }
  if (run.status !== 0 && errors.length === 0) errors.push(`cargo clippy exited with ${String(run.status)}`);
  return { warnings: [...warnings.values()], errors };
}

/** `cargo machete`'s complaint, or null when every dependency is used. */
export function runMachete(root: string): string | null {
  const run = spawnSync("cargo", ["machete", "--skip-target-dir", CRATE], { cwd: root, encoding: "utf8" });
  if (run.error) return `cargo machete couldn't run (cargo install cargo-machete): ${run.error.message}`;
  return run.status === 0 ? null : run.stdout + run.stderr;
}
