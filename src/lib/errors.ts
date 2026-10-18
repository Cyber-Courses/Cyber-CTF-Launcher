import type { CommandError } from "@/lib/tauri/generated";
import { translate, type MessageKey, type Vars } from "@/lib/i18n";

export type { CommandError };
/** What the Rust core says went wrong, as a code (src-tauri/src/error.rs `Code`). */
export type ErrorCode = CommandError["code"];

/** `t` from `useT()` in a component, `translate` elsewhere. */
type Translate = (key: MessageKey, vars?: Vars) => string;

/** The error a Tauri command rejected with, when `e` is one. */
export function commandError(e: unknown): CommandError | null {
  if (typeof e !== "object" || e === null) return null;
  const { code, message } = e as Record<string, unknown>;
  return typeof code === "string" && typeof message === "string" ? (e as CommandError) : null;
}

/** The code of a command's error; null for anything else (a JS error, a string). */
export const errorCode = (e: unknown): ErrorCode | null => commandError(e)?.code ?? null;

/** A call that needs the account found no session: the app is signed out. */
export function isSignedOut(e: unknown): boolean {
  const code = errorCode(e);
  return code === "signedOut" || code === "sessionExpired";
}

/**
 * What went wrong, in the player's language: a command's error by its code (its English message
 * for `other`), a JS error by its message (no "Error: " in front), anything else as text.
 */
export function describeError(e: unknown, t: Translate = translate): string {
  const known = commandError(e);
  if (known) return known.code === "other" ? known.message : t(`errors.codes.${known.code}`, "args" in known ? known.args : undefined);
  if (e instanceof Error) return e.message;
  return String(e);
}
