import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { eslintExceptions, linted } from "./eslint.ts";

// The disable comments, built from parts so this file holds none itself.
const DISABLE = ["eslint", "-disable"].join("");

describe("eslintExceptions", () => {
  it("lists each disable comment, with whether it says why", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "eslint-exceptions-"));
    const lines = [
      `// ${DISABLE}-next-line react-hooks/set-state-in-effect -- read once on mount`,
      `const a = 1; // ${DISABLE}-line no-console`,
      `/* ${DISABLE} max-lines */`,
      "// a comment that mentions eslint without disabling anything",
      "const b = 2;",
    ];
    fs.writeFileSync(path.join(root, "a.ts"), lines.join("\r\n"));
    const found = eslintExceptions(root, ["a.ts"]);
    expect(found.map((e) => [e.file, e.text, e.hasReason])).toEqual([
      ["a.ts", `${DISABLE}-next-line react-hooks/set-state-in-effect -- read once on mount`, true],
      ["a.ts", `${DISABLE}-line no-console`, false],
      ["a.ts", `${DISABLE} max-lines`, false],
    ]);
  });
});

describe("linted", () => {
  it("lints the folders pnpm lint names, the repository's own among them", () => {
    expect(linted(JSON.stringify({ scripts: { lint: "eslint src --cache e2e" } }))).toEqual(["src", "e2e"]);
    expect(linted(fs.readFileSync(path.join(import.meta.dirname, "../../package.json"), "utf8"))).toEqual(["src", "scripts", "e2e"]);
  });
});
