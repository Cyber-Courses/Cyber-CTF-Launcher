import { describe, expect, it } from "vitest";

import { header } from "./cli.ts";

describe("header", () => {
  it("is the comment a script starts with, without its slashes, up to the first line of code", () => {
    const source = "// Checks the links.\n//\n//   pnpm check:docs\nimport fs from 'node:fs';\n// not the header\n";
    expect(header(source)).toBe("Checks the links.\n\n  pnpm check:docs");
  });
});
