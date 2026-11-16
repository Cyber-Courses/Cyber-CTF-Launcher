import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

// support/owned.mjs run as the desktop runner runs it, against this machine's Docker: opt-in (the
// harness, `pnpm test:harness`), as it starts node and asks the real Docker CLI.

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

describe("the command line", () => {
  it("lists nothing as the run's for a run with no lab, and refuses a missing folder", () => {
    const root = mkdtempSync(join(tmpdir(), "cyberctf-e2e-test-"));
    dirs.push(root);
    const script = join(import.meta.dirname, "owned.mjs");
    // Whatever Docker runs here, none of it is the run's: the run installed no lab.
    const r = spawnSync(process.execPath, [script, "projects", root], { encoding: "utf8" });
    expect(r.status).toBe(0);
    expect(r.stdout).not.toMatch(/^ours/m);
    expect(spawnSync(process.execPath, [script, "projects"], { encoding: "utf8" }).status).toBe(2);
  });
});
