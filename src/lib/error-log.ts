import { appLogDir } from "@tauri-apps/api/path";
import { error as logError, warn as logWarn } from "@tauri-apps/plugin-log";

/** The error as text, with its stack when it has one. */
export function describeError(err: unknown): string {
  if (err instanceof Error) return err.stack && err.stack.includes(err.message) ? err.stack : `${err.message}\n${err.stack ?? ""}`.trim();
  if (typeof err === "string") return err;
  try {
    // `undefined`, functions and symbols have no JSON: `stringify` returns undefined for them.
    const json = JSON.stringify(err) as unknown;
    return typeof json === "string" ? json : String(err);
  } catch {
    return String(err);
  }
}

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Writes `[where] page: err` to the app's log file with `write`. Never throws. */
function writeLog(write: (message: string) => Promise<void>, where: string, err: unknown): void {
  if (!inTauri()) return;
  const page = window.location.pathname + window.location.search;
  try {
    write(`[${where}] ${page}: ${describeError(err)}`).catch(() => {});
  } catch {
    // The log command itself is unavailable (an old core); nothing else to do.
  }
}

/**
 * Writes a frontend error to the app's log file (cyberctf.log, beside the Rust side's lines), so
 * a window that crashed or went blank leaves a trace. Never throws: logging must not fail the
 * thing it reports on. Outside Tauri (the browser preview) it does nothing.
 */
export function logFrontendError(where: string, err: unknown): void {
  writeLog(logError, where, err);
}

/** [`logFrontendError`] at the warning level: a failure nobody is waiting on, or one the player
 *  was told about, kept in the log file for support (a release build has no console). */
export function logFrontendWarning(where: string, err: unknown): void {
  writeLog(logWarn, where, err);
}

/** The folder of the app's log files, for a bug report; null outside Tauri or when unknown. */
export async function logFolder(): Promise<string | null> {
  if (!inTauri()) return null;
  try {
    return await appLogDir();
  } catch {
    return null;
  }
}

let installed = false;

/** Logs uncaught errors and unhandled promise rejections in this window, once per window. */
export function installErrorLogging(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;
  window.addEventListener("error", (e) => {
    logFrontendError("uncaught", e.error ?? e.message);
  });
  window.addEventListener("unhandledrejection", (e) => {
    logFrontendError("unhandled rejection", e.reason);
  });
}
