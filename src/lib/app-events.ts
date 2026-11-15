import { getCurrentWindow } from "@tauri-apps/api/window";
import { warn } from "@/lib/failure";
import { DROP, channel, send, signal } from "@/lib/event-channel";

/**
 * Events between the app's windows (the main window and the Settings window), broadcast with
 * Tauri's event bus: `focus` doesn't fire reliably when moving between the app's own windows,
 * and Settings can't change the main window's screen or state directly.
 */
/** Sign-in changed in one window: the others read it again. */
export const AUTH_CHANGED_EVENT = "cyberctf:auth-changed";
/** Show a screen in the main window (payload: the tab), e.g. "machine" from Settings. */
export const NAVIGATE_EVENT = "cyberctf:navigate";
/** Show the onboarding again in the main window. */
export const REPLAY_ONBOARDING_EVENT = "cyberctf:replay-onboarding";

/** Sign-in changed in another window. */
export const authChanged = signal(AUTH_CHANGED_EVENT);
/** Another window asks the main one to show a screen: the screen's name (a string). */
export const navigate = channel<string>(NAVIGATE_EVENT, (tab) => (typeof tab === "string" ? tab : DROP));
/** Another window asks the main one to show the onboarding again. */
export const replayOnboarding = signal(REPLAY_ONBOARDING_EVENT);

/** Asks the main window to show `tab`, and closes this window when it is the Settings one. */
export function showInMainWindow(tab: string) {
  send(navigate, tab).catch(warn("asking the main window to switch screens"));
  closeIfSettings();
}

/** Closes this window when it is the Settings one (the main window stays). */
export function closeIfSettings() {
  const win = getCurrentWindow();
  if (win.label === "settings") win.close().catch(warn("closing the Settings window"));
}
