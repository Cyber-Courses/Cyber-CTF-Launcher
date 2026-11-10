import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { rustSources, scanRust } from "./rust-scan.ts";

/** `files` (path → text) written under a fresh temporary root's `src-tauri/src`; the root. */
function tree(files: Record<string, string>): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rust-scan-"));
  for (const [rel, text] of Object.entries(files)) {
    const full = path.join(root, "src-tauri/src", rel);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, text);
  }
  return root;
}

describe("rustSources", () => {
  it("reads every .rs file under the folder, by its repository path, in order", () => {
    const root = tree({ "lib.rs": "mod a;\r\nmod b;\n", "b/c.rs": "fn c() {}\n", "a.rs": "fn a() {}\n", "notes.md": "# not Rust\n" });
    const sources = rustSources(root, "src-tauri/src");
    expect(sources.map((s) => s.file)).toEqual(["src-tauri/src/a.rs", "src-tauri/src/b/c.rs", "src-tauri/src/lib.rs"]);
    expect(sources[2].all).toEqual(["mod a;", "mod b;", ""]);
  });
});

describe("scanRust", () => {
  const scan = (text: string) => scanRust([{ file: "src-tauri/src/x.rs", all: text.split("\n") }]);

  it("finds a comment that is a line of code, and leaves prose and doc comments alone", () => {
    const { findings } = scan(
      ["// let x = compute();", "// a sentence about the code;", "/// let doc = 1;", "//! use crate::x;", "// }", "// #[test]", "fn f() {}"].join("\n"),
    );
    expect(findings.map((f) => [f.rule, f.line, f.message])).toEqual([
      ["rust/commented-out-code", 1, "// let x = compute();"],
      ["rust/commented-out-code", 5, "// }"],
      ["rust/commented-out-code", 6, "// #[test]"],
    ]);
  });

  it("counts each lint exception, with whether it says why", () => {
    const { exceptions } = scan(
      [
        '#[allow(clippy::too_many_lines, reason = "a list")]',
        "// The OS hands it over by value.",
        "#[allow(clippy::needless_pass_by_value)]",
        "#[expect(dead_code)]",
        '#![cfg_attr(not(windows), allow(dead_code, reason = "Windows only"))]',
        "#[derive(Debug)]",
      ].join("\n"),
    );
    expect(exceptions.map((e) => [e.text, e.hasReason])).toEqual([
      ['#[allow(clippy::too_many_lines, reason = "a list")]', true],
      ["#[allow(clippy::needless_pass_by_value)]", true],
      ["#[expect(dead_code)]", false],
      ['#![cfg_attr(not(windows), allow(dead_code, reason = "Windows only"))]', true],
    ]);
    expect(new Set(exceptions.map((e) => e.file))).toEqual(new Set(["src-tauri/src/x.rs"]));
  });
});
