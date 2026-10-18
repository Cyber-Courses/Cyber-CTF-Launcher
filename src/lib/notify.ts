import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

/**
 * Fire a native OS notification, requesting permission on first use. Never throws:
 * notifications are a nicety, so a denied/unavailable permission is silently ignored.
 */
export async function notify(title: string, body?: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body });
  } catch {
    /* notifications unavailable - non-fatal */
  }
}
