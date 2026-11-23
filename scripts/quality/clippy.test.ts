import path from "node:path";
import { describe, expect, it } from "vitest";

import { clippyOutcome, macheteComplaint } from "./clippy.ts";

const ROOT = path.resolve("/repo");

/** One line of cargo's JSON output: a compiler message at `file`:`line` of the crate. */
function message(level: string, code: string | null, file: string, line: number, extra: object = {}): string {
  const span = { file_name: file, line_start: line, column_start: 5, is_primary: true };
  return JSON.stringify({
    reason: "compiler-message",
    message: { level, message: `${code ?? "a"} here`, rendered: `${level}: rendered`, code: code ? { code } : null, spans: [span], ...extra },
  });
}

describe("clippyOutcome", () => {
  it("counts each warning once by place, as clippy/ or rustc/, and keeps the errors", () => {
    const stdout = [
      '{"reason":"compiler-artifact"}',
      message("warning", "clippy::too_many_lines", "src/app.rs", 10),
      message("warning", "clippy::too_many_lines", "src/app.rs", 10),
      message("warning", "dead_code", "src/labs.rs", 3),
      message("error", "E0308", "src/labs.rs", 4),
      "   Compiling cyberctf",
    ].join("\n");
    const { warnings, errors } = clippyOutcome(ROOT, stdout, 101);
    expect(warnings).toEqual([
      { rule: "clippy/too_many_lines", file: "src-tauri/src/app.rs", line: 10, message: "clippy::too_many_lines here" },
      { rule: "rustc/dead_code", file: "src-tauri/src/labs.rs", line: 3, message: "dead_code here" },
    ]);
    expect(errors).toEqual(["error: rendered"]);
  });

  it("leaves out a lint-less note and a file outside the repository (a path dependency's own)", () => {
    const stdout = [message("warning", null, "src/app.rs", 1), message("warning", "unused_imports", "/elsewhere/isoloom/src/lib.rs", 2)].join("\n");
    expect(clippyOutcome(ROOT, stdout, 0)).toEqual({ warnings: [], errors: [] });
  });

  it("follows a macro's expansion back to the crate's own file", () => {
    const own = { file_name: "src/app.rs", line_start: 7, column_start: 1, is_primary: true };
    const inMacro = { file_name: "/rustc/library/core/src/macros.rs", line_start: 1, column_start: 1, is_primary: true, expansion: { span: own } };
    const line = JSON.stringify({
      reason: "compiler-message",
      message: { level: "warning", message: "m", rendered: null, code: { code: "clippy::panic" }, spans: [inMacro] },
    });
    expect(clippyOutcome(ROOT, line, 0).warnings).toEqual([{ rule: "clippy/panic", file: "src-tauri/src/app.rs", line: 7, message: "m" }]);
  });

  it("fails a run that exited badly without an error message", () => {
    expect(clippyOutcome(ROOT, "", 101).errors).toEqual(["cargo clippy exited with 101"]);
    expect(clippyOutcome(ROOT, "", null).errors).toEqual(["cargo clippy exited with null"]);
  });
});

describe("macheteComplaint", () => {
  it("passes a clean run, quotes a run that found unused crates, and says how to install it when it can't run", () => {
    expect(macheteComplaint({ status: 0, stdout: "", stderr: "" })).toBeNull();
    expect(macheteComplaint({ status: 1, stdout: "unused: serde_yaml\n", stderr: "" })).toBe("unused: serde_yaml\n");
    expect(macheteComplaint({ error: new Error("spawn cargo ENOENT"), status: null, stdout: "", stderr: "" })).toBe(
      "cargo machete couldn't run (cargo install cargo-machete): spawn cargo ENOENT",
    );
  });
});
