import { ask } from "@tauri-apps/plugin-dialog";
import { translate } from "@/lib/i18n";
import { tell } from "@/lib/failure";

/** What a destructive action asks before it runs: the dialog's title, what it does, and the
 *  label of the button that does it (Cancel is the other). */
export type Confirmation = { title: string; message: string; confirm: string };

// One question at a time: a second press (a double click) while one is open is a no.
let asking = false;

/**
 * Asks before a destructive action, in the system's own dialog (a window of its own, not a line
 * of the page): true only when the player chose `confirm`. Cancel, closing the dialog, a press
 * while a question is already open, or a dialog that couldn't open (told) all keep everything as
 * it is.
 */
export async function confirmDestructive({ title, message, confirm }: Confirmation): Promise<boolean> {
  if (asking) return false;
  asking = true;
  try {
    return await ask(message, { title, kind: "warning", okLabel: confirm, cancelLabel: translate("ui.confirm.cancel") });
  } catch (e) {
    tell(translate("ui.confirm.failed"))(e);
    return false;
  } finally {
    asking = false;
  }
}
