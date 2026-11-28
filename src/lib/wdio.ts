/**
 * The GUI e2e's hooks: whether this build carries them, and what the specs answer through them.
 *
 * A development build (the desktop e2e's debug app loads `next dev`) and one built with
 * `NEXT_PUBLIC_WDIO=1` (a release app under the e2e) carry them; both are inlined at build time,
 * so a production build without the flag drops the hooks and their code. The specs set each
 * hook as a `__wdio_*` localStorage key (tests/e2e/desktop/support/app.mjs), which every window of
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

/**
 * The property a terminal's element carries in a build with the hooks: what the terminal shows,
 * read from xterm's buffer, one line per row of its viewport (tests/e2e/desktop/support/page.mjs,
 * `terminalText`). xterm draws its rows in animation frames, which a webview doesn't run while
 * the screen is locked or the display asleep: the rows stay blank there, the buffer doesn't.
 */
export const WDIO_TERMINAL_TEXT = "__wdioTerminalText";

/** The part of an xterm.js terminal its text is read from. */
type TerminalBuffer = {
  rows: number;
  buffer: { active: { viewportY: number; getLine(y: number): { translateToString(trimRight?: boolean): string } | undefined } };
};

/** In a build with the hooks, lets the specs read `term`'s text on `element`
 *  ([`WDIO_TERMINAL_TEXT`]); what takes it away again. Nothing without the hooks. */
export function exposeTerminalText(element: HTMLElement, term: TerminalBuffer): () => void {
  if (!WDIO_HOOKS) return () => {};
  const read = () => {
    const { viewportY } = term.buffer.active;
    return Array.from({ length: term.rows }, (_, row) => term.buffer.active.getLine(viewportY + row)?.translateToString(true) ?? "").join("\n");
  };
  Object.defineProperty(element, WDIO_TERMINAL_TEXT, { value: read, configurable: true });
  return () => {
    Reflect.deleteProperty(element, WDIO_TERMINAL_TEXT);
  };
}
