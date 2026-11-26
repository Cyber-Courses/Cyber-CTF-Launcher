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
// name it. A tool's own cache there (Node's compile cache) isn't a leftover, nor the folder the
// runner service keeps its socket in for a home on a long path (`isothrum-<uid>`).
//
// Knobs:
//   CYBERCTF_TEST_TEMP_KEEP   keep the folder at the end, its path printed, to look into what was left
//
// Exit status: the first failing command's, else 1 when something was left, 0 when nothing was;
// 2 when a command couldn't start.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
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

/**
 * The folder of the runner services' sockets for homes on paths too long for one
 * (`isothrum-<uid>`, Isothrum's): kept on purpose, across runs, like a tool's cache.
 */
export const SOCKET_FOLDER = /^isothrum-\d+$/;

/** What is left at the top of `dir`, sorted, the tools' caches and the runner's socket folder aside. */
export function leftovers(dir: string): string[] {
  return readdirSync(dir)
    .filter((name) => !TOOL_CACHES.has(name) && !SOCKET_FOLDER.test(name))
    .sort();
}

/**
 * Whether `program` starts through cmd on `platform`. On Windows a `.cmd` shim (pnpm's) needs it;
 * a program that is an `.exe` (node, bash, cargo; found on `path`, or named by its path) doesn't,
 * and must not: cmd would split its arguments again on their spaces, quotes and new lines (a
 * `node -e` script, a `bash -c` function).
 */
export function needsShell(program: string, platform: NodeJS.Platform = process.platform, path = process.env.PATH ?? ""): boolean {
  if (platform !== "win32") return false;
  if (/[\\/]/.test(program)) return /\.(cmd|bat)$/i.test(program);
  const exe = /\.exe$/i.test(program) ? program : `${program}.exe`;
  return !path.split(";").some((dir) => dir !== "" && existsSync(join(dir, exe)));
}

/** Runs `commands` one after the other with a fresh temp folder; the exit status. */
export function run(commands: readonly (readonly string[])[], keep: boolean, base = tempBase()): number {
  const dir = mkdtempSync(join(base, "cyberctf-tests-"));
  let status = 0;
  for (const [program, ...args] of commands) {
    const ran = spawnSync(program, args, { stdio: "inherit", env: withTemp(process.env, dir), shell: needsShell(program) });
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
