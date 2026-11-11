// The browser preview: the UI in a plain browser, every Tauri command it calls answered by the dev
// mock (src/lib/dev-mock/) with sample data. The app's own dev server (`pnpm dev`, the one
// `pnpm tauri dev` loads) never serves the mock: a browser gets a notice there.
//
//   pnpm dev:web                  then http://127.0.0.1:3207/?mock
//   pnpm dev:web --port 3210      on another port (Playwright's, playwright.config.ts)
//
// It runs `next dev` built with NEXT_PUBLIC_DEV_MOCK=1 (next.config.ts), in its own folder
// (.next-web, so it runs beside `pnpm tauri dev`), listening on the loopback interface only.
import { spawn } from "node:child_process";
import { createRequire } from "node:module";

import { helpAsked } from "../lib/cli.ts";

/** The preview's port when none is given: the app's dev server has 3206 (tauri.conf.json). */
const DEFAULT_PORT = 3207;

/** `next dev`'s arguments for the preview, from the script's own (`--port <n>` alone); throws on
 *  anything else, a hostname above all: the preview listens on 127.0.0.1 and nowhere else. */
export function nextDevArgs(args: readonly string[]): string[] {
  let port = String(DEFAULT_PORT);
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    const value = arg === "--port" || arg === "-p" ? args[++i] : /^--port=(.*)$/.exec(arg)?.[1];
    if (value === undefined) throw new Error(`pnpm dev:web takes --port <n> alone, not ${arg}`);
    if (!/^\d{1,5}$/.test(value) || Number(value) < 1 || Number(value) > 65535) throw new Error(`not a port: ${value}`);
    port = value;
  }
  return ["dev", "--hostname", "127.0.0.1", "--port", port];
}

/** The environment `next dev` runs in: the caller's, with the mock built in. */
export function previewEnv<E extends Record<string, string | undefined>>(env: E): E & { NEXT_PUBLIC_DEV_MOCK: "1" } {
  return { ...env, NEXT_PUBLIC_DEV_MOCK: "1" };
}

if (process.argv[1]?.endsWith("web.ts") && !helpAsked(import.meta.url, process.argv.slice(2))) {
  let args: string[];
  try {
    args = nextDevArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  // Next's own entry point, run by this Node: no shell, the same on Windows.
  const next = createRequire(import.meta.url).resolve("next/dist/bin/next");
  const child = spawn(process.execPath, [next, ...args], { stdio: "inherit", env: previewEnv(process.env) });
  for (const signal of ["SIGINT", "SIGTERM"] as const) process.on(signal, () => child.kill(signal));
  child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
}
