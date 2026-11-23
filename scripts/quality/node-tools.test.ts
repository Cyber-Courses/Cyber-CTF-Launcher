import path from "node:path";
import { describe, expect, it } from "vitest";

import { jscpdFindings, knipFindings } from "./node-tools.ts";

const ROOT = path.resolve("/repo");

describe("jscpdFindings", () => {
  it("puts each clone at its first copy, naming the second", () => {
    const report = {
      duplicates: [{ firstFile: { name: path.join(ROOT, "src/a.ts"), start: 3 }, secondFile: { name: path.join(ROOT, "src/b.ts"), start: 9 }, lines: 12 }],
    };
    expect(jscpdFindings(ROOT, report)).toEqual([{ rule: "dup/clone", file: "src/a.ts", line: 3, message: "12 lines also at src/b.ts:9" }]);
  });
});

describe("knipFindings", () => {
  it("makes one finding per item of each kind, at its line or the file's first, and skips the issue's other fields", () => {
    const report = { issues: [{ file: "src/a.ts", owners: "x", exports: [{ name: "unused", line: 4 }], files: [{}], types: [] }] };
    expect(knipFindings(report)).toEqual([
      { rule: "knip/exports", file: "src/a.ts", line: 4, message: "unused" },
      { rule: "knip/files", file: "src/a.ts", line: 1, message: "files" },
    ]);
  });
});
