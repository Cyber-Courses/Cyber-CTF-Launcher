/**
 * The GUI e2e's hooks: whether this build carries them, and what the specs answer through them.
 *
 * A development build (the desktop e2e's debug app loads `next dev`) and one built with
 * `NEXT_PUBLIC_WDIO=1` (a release app under the e2e) carry them; both are inlined at build time,
 * so a production build without the flag drops the hooks and their code. The specs set each
 * hook as a `__wdio_*` localStorage key (tests/desktop/lib/support.mjs), which every window of
 * the app reads: the system's own dialogs (a folder picker, a confirmation) are windows WebDriver
 * can't drive, so a build with the hooks answers them from the key instead of opening them.
 */
export const WDIO_HOOKS = process.env.NODE_ENV !== "production" || process.env.NEXT_PUBLIC_WDIO === "1";

/** The specs' answer under `key`, in a build with the hooks; null without one, or without the hooks. */
export function wdioAnswer(key: string): string | null {
  if (!WDIO_HOOKS) return null;
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** The path the specs answer every file and folder picker with (`answerPickers`). */
export const WDIO_PICK = "__wdio_pick__";
/** How the specs answer every destructive confirmation (`confirmSystemDialogs`): `confirm` or `cancel`. */
export const WDIO_CONFIRM = "__wdio_confirm__";
