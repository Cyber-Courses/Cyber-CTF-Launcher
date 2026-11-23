import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { enableHooks, wanted } from "./hooks.ts";

describe("the git hooks' setup (pnpm install's prepare)", () => {
  it("is for a git checkout outside CI", () => {
    expect(wanted({}, true)).toBe(true);
    expect(wanted({ CI: "true" }, true)).toBe(false);
    expect(wanted({}, false)).toBe(false);
  });

  it("points git at .githooks, and says what git said when it couldn't", () => {
    const dir = mkdtempSync(join(tmpdir(), "launcher-hooks-"));
    try {
      execFileSync("git", ["init", "-q", dir]);
      expect(enableHooks(dir)).toBeUndefined();
      expect(execFileSync("git", ["-C", dir, "config", "core.hooksPath"], { encoding: "utf8" }).trim()).toBe(".githooks");
      expect(enableHooks(join(dir, "no-such-folder"))).toBeDefined();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
