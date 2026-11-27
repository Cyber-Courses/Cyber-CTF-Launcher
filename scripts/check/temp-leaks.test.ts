import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { leftovers, needsShell, run, tempBase, withTemp } from "./temp-leaks.ts";

const bases: string[] = [];
afterEach(() => {
  vi.restoreAllMocks();
  for (const base of bases.splice(0)) rmSync(base, { recursive: true, force: true });
});

/** A fresh folder for the runs' temp folders, removed after the test. */
function base(): string {
  const dir = mkdtempSync(join(tmpdir(), "temp-leaks-test-"));
  bases.push(dir);
  return dir;
}

/** `node -e script`, as a test command. */
const node = (script: string) => [process.execPath, "-e", script];

describe("temp-leaks", () => {
  it("names what a run left at the top of its folder, sorted, and not a tool's cache nor the runner's socket folder", () => {
    const dir = base();
    mkdirSync(join(dir, "cyberctf-lab-x"));
    writeFileSync(join(dir, ".tmpA"), "");
    mkdirSync(join(dir, "node-compile-cache"));
    mkdirSync(join(dir, "isothrum-501"));
    mkdirSync(join(dir, "isothrum-x"));
    expect(leftovers(dir)).toEqual([".tmpA", "cyberctf-lab-x", "isothrum-x"]);
  });

  it("gives every platform's variable the folder, under /tmp off Windows", () => {
    expect(withTemp({ PATH: "p" }, "/t")).toEqual({ PATH: "p", TMPDIR: "/t", TMP: "/t", TEMP: "/t" });
    expect(tempBase("darwin")).toBe("/tmp");
    expect(tempBase("linux")).toBe("/tmp");
    expect(tempBase("win32")).toBe(tmpdir());
  });

  it("starts a Windows .cmd shim through cmd, and an .exe as it is, its arguments untouched", () => {
    const dir = base();
    writeFileSync(join(dir, "node.exe"), "");
    writeFileSync(join(dir, "pnpm.cmd"), "");
    const path = `;${dir}`;
    expect(needsShell("node", "win32", path)).toBe(false);
    expect(needsShell("node.exe", "win32", path)).toBe(false);
    expect(needsShell("pnpm", "win32", path)).toBe(true);
    expect(needsShell(String.raw`C:\Program Files\nodejs\node.exe`, "win32", path)).toBe(false);
    expect(needsShell("C:/tools/pnpm.cmd", "win32", path)).toBe(true);
    expect(needsShell("pnpm", "linux", path)).toBe(false);
  });

  it("passes a run that removes what it made, and fails one that leaves something, removing the folder either way", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "log").mockImplementation(() => {});
    const dir = base();
    const clean = "const p = require('path').join(require('os').tmpdir(), 'x'); require('fs').mkdirSync(p); require('fs').rmSync(p, { recursive: true });";
    expect(run([node(clean)], false, dir)).toBe(0);
    const leaky = "require('fs').mkdirSync(require('path').join(process.env.TMPDIR, 'cyberctf-lab-1'));";
    expect(run([node(leaky)], false, dir)).toBe(1);
    expect(console.error).toHaveBeenCalledWith("check:temp-leaks: a test left cyberctf-lab-1 in its temp folder");
    expect(readdirSync(dir)).toEqual([]);
  });

  it("keeps a failing command's status, and the folder when asked", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const dir = base();
    expect(run([node("process.exit(3)"), node("process.exit(0)")], true, dir)).toBe(3);
    const kept = readdirSync(dir);
    expect(kept).toHaveLength(1);
    expect(existsSync(join(dir, kept[0]))).toBe(true);
  });
});
