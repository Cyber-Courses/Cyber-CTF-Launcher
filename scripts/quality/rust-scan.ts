// Rules over the Rust sources that clippy has no lint for, read line by line: commented-out code,
// and the local lint exceptions the ratchet counts. The rules that read a file's split into shipped
// and test code (its length, `tauri` outside the adapter layer, the module convention) are the
// crate's own tests, in `src-tauri/src/testing/source_scan.rs`, beside that split.

import fs from "node:fs";
import path from "node:path";

import type { Exception, Finding } from "./model.ts";

/** A `//` comment (not a doc comment) that is a line of code. */
const COMMENTED_CODE = [
  /^(let|fn|pub|use|mod|impl|struct|enum|const|static|match|return|if|else|for|while|loop)\b.*[;{]$/,
  /^[A-Za-z_][\w:.&*]*(\(.*\))?(\.\w+\(.*\))*;$/,
  /^[)}\]];?$/,
  /^#!?\[.*\]$/,
];

/** An `#[allow(…)]`/`#[expect(…)]`, also inside a `#[cfg_attr(…)]`. */
const ATTRIBUTE = /#!?\[(cfg_attr\(.*\b)?(allow|expect)\(.*\)\]/;

type Source = { file: string; all: string[] };

/** Every `.rs` file under `dir` (repository-relative `rel`), as its lines. */
export function rustSources(root: string, rel: string): Source[] {
  const out: Source[] = [];
  const walk = (dir: string): void => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".rs"))
        out.push({ file: path.relative(root, full).split(path.sep).join("/"), all: fs.readFileSync(full, "utf8").split(/\r?\n/) });
    }
  };
  walk(path.join(root, rel));
  return out.sort((a, b) => a.file.localeCompare(b.file));
}

/** The findings and exceptions of the Rust sources. */
export function scanRust(sources: readonly Source[]): { findings: Finding[]; exceptions: Exception[] } {
  const findings: Finding[] = [];
  const exceptions: Exception[] = [];
  for (const s of sources) scanLines(s, findings, exceptions);
  return { findings, exceptions };
}

function scanLines(s: Source, findings: Finding[], exceptions: Exception[]): void {
  s.all.forEach((raw, i) => {
    const line = raw.trim();
    const comment = /^\/\/(?![/!])\s?(.*)$/.exec(line);
    if (comment && COMMENTED_CODE.some((r) => r.test(comment[1].trim()))) {
      findings.push({ rule: "rust/commented-out-code", file: s.file, line: i + 1, message: line });
    }
    const attr = ATTRIBUTE.exec(line);
    if (attr) {
      const above = (s.all[i - 1] ?? "").trim();
      exceptions.push({ file: s.file, text: attr[0], hasReason: /reason\s*=/.test(attr[0]) || above.startsWith("//") });
    }
  });
}
