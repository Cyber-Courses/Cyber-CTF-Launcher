import { open } from "@tauri-apps/plugin-dialog";
import { WDIO_PICK, wdioAnswer } from "@/lib/wdio";

/** What a picker is told: its title, and where it opens. */
export type PickerOptions = { title: string; defaultPath?: string };

/**
 * The system's folder picker: the folder chosen, or null when the player closed it. Under the
 * GUI e2e (a build with the hooks, `WDIO_PICK` set by `answerPickers`) the specs' path, with no
 * picker opened: the system's picker is a window WebDriver can't drive, and the webview's
 * `invoke` can't be patched from outside (Tauri defines it read-only).
 */
export async function pickFolder(options: PickerOptions): Promise<string | null> {
  const answered = wdioAnswer(WDIO_PICK);
  if (answered !== null) return answered;
  const picked = await open({ ...options, directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

/** The system's file picker, as [`pickFolder`] is the folder one. */
export async function pickFile(options: PickerOptions): Promise<string | null> {
  const answered = wdioAnswer(WDIO_PICK);
  if (answered !== null) return answered;
  const picked = await open({ ...options, directory: false, multiple: false });
  return typeof picked === "string" ? picked : null;
}
