import { readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * The commands the Rust core registers: build.rs's `COMMANDS`, the list the app's permissions
 * are made from (a Rust test holds it to `generate_handler!`). A test double or the dev mock
 * answering a command that isn't in it answers one the app can't call: the two drifted apart.
 */
export function backendCommands(): Set<string> {
  const build = readFileSync(join(import.meta.dirname, "../../src-tauri/build.rs"), "utf8");
  const list = /const COMMANDS: &\[&str\] = &\[([^\]]*)\];/.exec(build)?.[1] ?? "";
  return new Set([...list.matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]));
}

/** Whether `cmd` is one the app can invoke: a registered command, or a Tauri plugin's. */
export const callable = (registered: Set<string>, cmd: string) => cmd.startsWith("plugin:") || registered.has(cmd);
