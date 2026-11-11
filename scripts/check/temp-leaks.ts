// Runs the unit tests with a temp folder of their own and fails when they leave anything in it:
// a test removes the folders and files it makes (a Rust test through `testing::temp_dir`, a
// TypeScript one in `afterEach` or `onTestFinished`), so none piles up in the developer's temp
// folder run after run. CI's Linux legs and `pnpm check` run the unit tests through it.
//
//   pnpm check:temp-leaks                                                   the web and Rust unit tests
//   pnpm check:temp-leaks cargo test --manifest-path src-tauri/Cargo.toml   one command, any test command
//
// The folder is `/tmp/cyberctf-tests-XXXXXX` on macOS and Linux (a short path: a test's Unix
// socket must fit in 104 bytes), under the system's temp folder on Windows; TMPDIR, TMP and TEMP
// name it. A tool's own cache there (Node's compile cache) isn't a leftover.
//
// Knobs:
//   CYBERCTF_TEST_TEMP_KEEP   keep the folder at the end, its path printed, to look into what was left
//
// Exit status: the first failing command's, else 1 when something was left, 0 when nothing was;
// 2 when a command couldn't start.
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { helpAsked } from "../lib/cli.ts";

/** What tools keep in the temp folder on purpose, across runs: not a test's leftover. */
export const TOOL_CACHES: ReadonlySet<string> = new Set(["node-compile-cache"]);

/** The unit tests, as `pnpm test` runs them. */
export const UNIT_TESTS: readonly (readonly string[])[] = [
  ["pnpm", "exec", "vitest", "run"],
  ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml"],
];

/** Where the run's temp folder goes on `platform`. */
export function tempBase(platform: NodeJS.Platform = process.platform): string {
  return platform === "win32" ? tmpdir() : "/tmp";
}

/** `env` with `dir` as its temp folder, whichever variable a platform's tools read. */
export function withTemp<E extends Readonly<Record<string, string | undefined>>>(env: E, dir: string): E & { TMPDIR: string; TMP: string; TEMP: string } {
  return { ...env, TMPDIR: dir, TMP: dir, TEMP: dir };
}

/** What is left at the top of `dir`, sorted, the tools' caches aside. */
export function leftovers(dir: string): string[] {
  return readdirSync(dir)
    .filter((name) => !TOOL_CACHES.has(name))
    .sort();
}

/** Runs `commands` one after the other with a fresh temp folder; the exit status. */
export function run(commands: readonly (readonly string[])[], keep: boolean, base = tempBase()): number {
  const dir = mkdtempSync(join(base, "cyberctf-tests-"));
  let status = 0;
  for (const [program, ...args] of commands) {
    const ran = spawnSync(program, args, { stdio: "inherit", env: withTemp(process.env, dir), shell: process.platform === "win32" });
    if (ran.error) {
      console.error(`check:temp-leaks: ${program} couldn't start: ${ran.error.message}`);
      status = 2;
      break;
    }
    if (ran.status !== 0 && status === 0) status = ran.status ?? 1;
  }
  const left = leftovers(dir);
  for (const name of left) console.error(`check:temp-leaks: a test left ${name} in its temp folder`);
  if (keep) console.error(`check:temp-leaks: the tests' temp folder is kept (CYBERCTF_TEST_TEMP_KEEP): ${dir}`);
  else rmSync(dir, { recursive: true, force: true });
  if (status === 0 && left.length > 0) status = 1;
  if (status === 0) console.log("check:temp-leaks: nothing left in the temp folder");
  return status;
}

const args = process.argv.slice(2);
if (process.argv[1]?.endsWith("temp-leaks.ts") && !helpAsked(import.meta.url, args)) {
  process.exit(run(args.length > 0 ? [args] : UNIT_TESTS, Boolean(process.env.CYBERCTF_TEST_TEMP_KEEP)));
}
