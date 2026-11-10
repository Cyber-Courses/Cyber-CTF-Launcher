import { ignore, warn } from "@/lib/failure-log";

// This machine's preferences in the webview's localStorage, and what a window keeps for its
// session in sessionStorage. Storage can be unavailable (a private window, blocked site data): a
// read then finds nothing, and a write lasts only as long as the value it set is kept in memory by
// its caller.

/** The value under `key` in `storage` (read inside the guard: the global itself can throw); null
 *  when there is none, `unreadable` when storage can't be read. */
function readFrom(storage: () => Storage, key: string, unreadable: string | null): string | null {
  try {
    return storage().getItem(key);
  } catch (e) {
    return ignore("no storage here: nothing saved", () => unreadable)(e);
  }
}

/** The value saved under `key`; null when there is none, `unreadable` when storage can't be
 *  read (none either, unless the caller says what that means). */
export function readStored(key: string, unreadable: string | null = null): string | null {
  return readFrom(() => localStorage, key, unreadable);
}

/** The value this window keeps under `key` for its session (it survives a reload); null when there
 *  is none, or session storage can't be read. */
export function readSession(key: string): string | null {
  return readFrom(() => sessionStorage, key, null);
}

/** Keeps `value` under `key` for this window's session. Storage that refuses is no failure: the
 *  caller keeps the value in memory, for this page. */
export function writeSession(key: string, value: string) {
  try {
    sessionStorage.setItem(key, value);
  } catch (e) {
    ignore("no session storage: kept in memory for this page")(e);
  }
}

/** The value saved under `key` when it is one of `allowed`; null otherwise (none, or one a
 *  later or earlier version saved, or a hand edit). */
export function readStoredOneOf<T extends string>(key: string, allowed: readonly T[]): T | null {
  return oneOf(readStored(key), allowed);
}

/** `value` (a stored one, or one another window just stored) when it is one of `allowed`; null
 *  otherwise. */
export function oneOf<T extends string>(value: string | null, allowed: readonly T[]): T | null {
  return value !== null && (allowed as readonly string[]).includes(value) ? (value as T) : null;
}

/** Saves `value` under `key`; null removes it. Storage that refuses is logged: the choice then
 *  isn't kept for the next start. */
export function writeStored(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch (e) {
    warn(`saving ${key}`)(e);
  }
}
