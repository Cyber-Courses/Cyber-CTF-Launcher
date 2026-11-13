import { createAsk } from "@/lib/ask";
import { WDIO_CONFIRM, wdioAnswer } from "@/lib/wdio";

/** What a destructive action asks before it runs: the dialog's title, what it does, and the
 *  label of the button that does it (Cancel is the other). */
export type Confirmation = { title: string; message: string; confirm: string };

/** What the player must read before going on: a title and what happened. */
export type Warning = { title: string; message: string };

/** The questions the app shell's `DestructivePrompt` shows; closing one is a no. */
export const destructive = createAsk<Confirmation, boolean>(false);

/** The warnings the app shell's `WarningPrompt` shows; closing one is having read it. */
export const warnings = createAsk<Warning, null>(null);

// One question at a time: a second press (a double click) while one is open is a no.
let asking = false;

/**
 * Asks before a destructive action, in the app's own dialog: true only when the player chose
 * `confirm`. Cancel, Escape, a click outside, or a press while a question is already open keep
 * everything as it is. Under the GUI e2e (a build with the hooks, `WDIO_CONFIRM` set by
 * `confirmSystemDialogs`) the specs' answer, with no dialog shown.
 */
export async function confirmDestructive(question: Confirmation): Promise<boolean> {
  if (asking) return false;
  asking = true;
  try {
    const answered = wdioAnswer(WDIO_CONFIRM);
    if (answered !== null) return answered === "confirm";
    return await destructive.ask(question);
  } finally {
    asking = false;
  }
}

/**
 * Says something the player must read before going on (what an action couldn't finish), in the
 * app's own dialog, which stays until it is closed: a toast would be gone before a list of paths
 * is read.
 */
export async function warnInDialog(warning: Warning): Promise<void> {
  await warnings.ask(warning);
}
