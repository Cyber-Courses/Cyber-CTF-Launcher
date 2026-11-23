import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { warn } from "@/lib/failure-log";

/**
 * Fire a native OS notification, requesting permission on first use. Never throws: a permission
 * the player denied shows nothing, and one that can't be shown is logged, as the core's are (with
 * the window in the background, "lab ready" may be the only word the player gets).
 */
export async function notify(title: string, body?: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body });
  } catch (e) {
    warn(`the notification "${title}" couldn't be shown`)(e);
  }
}
