// Turns the repository's git hooks on: core.hooksPath = .githooks (CONTRIBUTING.md, "Git hooks").
// `pnpm install` runs it (package.json's prepare), so a clone has the hooks from its first install
// on; the worktrees of a checkout share the setting. It does nothing in CI (CI is set: those
// clones commit nothing by hand) or outside a git checkout (a source archive).
//
//   pnpm run prepare        by hand, in a clone that had no pnpm install yet
import { spawnSync } from "node:child_process";

import { helpAsked } from "../lib/cli.ts";

/** Whether the hooks are to be set here: not in CI, and only in a git checkout. */
export function wanted(env: Readonly<Record<string, string | undefined>>, inGitCheckout: boolean): boolean {
  return !env.CI && inGitCheckout;
}

/** `git config core.hooksPath .githooks` in `cwd`; what git said when it refused, else nothing. */
export function enableHooks(cwd: string): string | undefined {
  const git = spawnSync("git", ["config", "core.hooksPath", ".githooks"], { cwd, encoding: "utf8" });
  if (git.status === 0) return undefined;
  // No stderr when git couldn't be started (no such folder, no git): the error says why.
  return git.error?.message ?? (git.stderr.trim() || `git exited with ${String(git.status)}`);
}

if (process.argv[1]?.endsWith("hooks.ts") && !helpAsked(import.meta.url, process.argv.slice(2))) {
  const inGitCheckout = spawnSync("git", ["rev-parse", "--git-dir"], { stdio: "ignore" }).status === 0;
  if (wanted(process.env, inGitCheckout)) {
    const refused = enableHooks(process.cwd());
    if (refused !== undefined) {
      console.error(`the git hooks aren't on: ${refused}`);
      process.exit(1);
    }
  }
}
