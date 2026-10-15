// Rules over the Rust sources that clippy has no lint for: file length, `tauri` outside the
// adapter layer, the module convention, commented-out code, and the local lint exceptions.

import fs from "node:fs";
import path from "node:path";

import type { Exception, Finding } from "./model.ts";

/** Lines of code a source file may have before its tests (blank and comment lines not counted). */
const MAX_FILE_LOC = 400;

/** Whole files of test code. */
const TEST_FILES = new Set(["tests.rs", "proptests.rs", "fake.rs", "test_app.rs", "proptest_support.rs", "source_scan.rs", "real_labs.rs"]);

/** Where `tauri` may be used: the crate root, the app shell and the command adapters. */
const TAURI_LAYERS = [/^src-tauri\/src\/(lib|main)\.rs$/, /^src-tauri\/src\/(app|commands)(\.rs$|\/)/];

const TAURI = /\btauri(_plugin_\w+)?::|\b(AppHandle|WebviewWindow|AppRuntime)\b/;

/** A `//` comment (not a doc comment) that is a line of code. */
const COMMENTED_CODE = [
  /^(let|fn|pub|use|mod|impl|struct|enum|const|static|match|return|if|else|for|while|loop)\b.*[;{]$/,
  /^[A-Za-z_][\w:.&*]*(\(.*\))?(\.\w+\(.*\))*;$/,
  /^[)}\]];?$/,
  /^#!?\[.*\]$/,
];

/** An `#[allow(…)]`/`#[expect(…)]`, also inside a `#[cfg_attr(…)]`. */
const ATTRIBUTE = /#!?\[(cfg_attr\(.*\b)?(allow|expect)\(.*\)\]/;

type Source = { file: string; code: string[]; all: string[] };

/** Every `.rs` file under `dir` (repository-relative `rel`), with its shipped code: the lines
 *  before its `#[cfg(test)] mod … {`, or none for a file of test code. */
export function rustSources(root: string, rel: string): Source[] {
  const out: Source[] = [];
  const walk = (dir: string): void => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".rs")) out.push(read(root, full, entry.name));
    }
  };
  walk(path.join(root, rel));
  return out.sort((a, b) => a.file.localeCompare(b.file));
}

function read(root: string, full: string, name: string): Source {
  const all = fs.readFileSync(full, "utf8").split(/\r?\n/);
  const file = path.relative(root, full).split(path.sep).join("/");
  if (TEST_FILES.has(name) || file.includes("/testing/")) return { file, code: [], all };
  const start = all.findIndex((line, i) => {
    const attr = line.trim();
    const next = (all[i + 1] ?? "").trim();
    return (attr === "#[cfg(test)]" || attr.startsWith("#[cfg(all(test")) && /^(pub\(crate\) )?mod \w+ \{$/.test(next);
  });
  return { file, code: start < 0 ? all : all.slice(0, start), all };
}

/** Non-blank lines that aren't only a comment. */
const loc = (lines: readonly string[]): number => lines.filter((l) => l.trim() !== "" && !l.trim().startsWith("//")).length;

/** The findings and exceptions of the Rust sources. */
export function scanRust(sources: readonly Source[]): { findings: Finding[]; exceptions: Exception[] } {
  const findings: Finding[] = [];
  const exceptions: Exception[] = [];
  for (const s of sources) {
    const lines = loc(s.code);
    if (lines > MAX_FILE_LOC) findings.push({ rule: "rust/file-over-400-loc", file: s.file, line: 1, message: `${String(lines)} lines of code` });
    if (path.basename(s.file) === "mod.rs") findings.push({ rule: "rust/mod-rs", file: s.file, line: 1, message: "use foo.rs next to foo/, not foo/mod.rs" });
    const tauri = s.code.findIndex((l) => TAURI.test(l) && !l.trim().startsWith("//"));
    if (tauri >= 0 && !TAURI_LAYERS.some((r) => r.test(s.file))) {
      findings.push({ rule: "rust/tauri-outside-adapters", file: s.file, line: tauri + 1, message: "domain code uses tauri: move it into commands/ or app/" });
    }
    scanLines(s, findings, exceptions);
  }
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
