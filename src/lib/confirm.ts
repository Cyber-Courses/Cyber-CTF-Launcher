import { ask, message as say } from "@tauri-apps/plugin-dialog";
import { translate } from "@/lib/i18n";
import { tell } from "@/lib/failure";
import { WDIO_CONFIRM, wdioAnswer } from "@/lib/wdio";

/** What a destructive action asks before it runs: the dialog's title, what it does, and the
 *  label of the button that does it (Cancel is the other). */
export type Confirmation = { title: string; message: string; confirm: string };

// One question at a time: a second press (a double click) while one is open is a no.
let asking = false;

/**
 * Asks before a destructive action, in the system's own dialog (a window of its own, not a line
 * of the page): true only when the player chose `confirm`. Cancel, closing the dialog, a press
 * while a question is already open, or a dialog that couldn't open (told) all keep everything as
 * it is. Under the GUI e2e (a build with the hooks, `WDIO_CONFIRM` set by
 * `confirmSystemDialogs`) the specs' answer, with no dialog opened: the system's dialog is a
 * window WebDriver can't press.
 */
export async function confirmDestructive({ title, message, confirm }: Confirmation): Promise<boolean> {
  if (asking) return false;
  asking = true;
  try {
    const answered = wdioAnswer(WDIO_CONFIRM);
    if (answered !== null) return answered === "confirm";
    return await ask(message, { title, kind: "warning", okLabel: confirm, cancelLabel: translate("ui.confirm.cancel") });
  } catch (e) {
    tell(translate("ui.confirm.failed"))(e);
    return false;
  } finally {
    asking = false;
  }
}

/**
 * Says something the player must read before going on (what an action couldn't finish), in the
 * system's dialog, which stays until it is closed: a toast would be gone before a list of paths
 * is read. One that couldn't open is told as a toast instead.
 */
export async function warnInDialog({ title, message }: { title: string; message: string }): Promise<void> {
  try {
    await say(message, { title, kind: "warning" });
  } catch (e) {
    tell(title)(e);
  }
}
