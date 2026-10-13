import { error as logError } from "@tauri-apps/plugin-log";

/** The error as text, with its stack when it has one. */
export function describeError(err: unknown): string {
  if (err instanceof Error) return err.stack && err.stack.includes(err.message) ? err.stack : `${err.message}\n${err.stack ?? ""}`.trim();
  if (typeof err === "string") return err;
  try {
    return JSON.stringify(err) ?? String(err);
  } catch {
    return String(err);
  }
}

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Writes a frontend error to the app's log file (cyberctf.log, beside the Rust side's lines), so
 * a window that crashed or went blank leaves a trace. Never throws: logging must not fail the
 * thing it reports on. Outside Tauri (the browser preview) it does nothing.
 */
export function logFrontendError(where: string, err: unknown): void {
  if (!inTauri()) return;
  const page = window.location.pathname + window.location.search;
  try {
    logError(`[${where}] ${page}: ${describeError(err)}`).catch(() => {});
  } catch {
    // The log command itself is unavailable (an old core); nothing else to do.
  }
}

let installed = false;

/** Logs uncaught errors and unhandled promise rejections in this window, once per window. */
export function installErrorLogging(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;
  window.addEventListener("error", (e) => logFrontendError("uncaught", e.error ?? e.message));
  window.addEventListener("unhandledrejection", (e) => logFrontendError("unhandled rejection", e.reason));
}
