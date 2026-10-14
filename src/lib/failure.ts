import { openUrl } from "@tauri-apps/plugin-opener";
import { notify } from "@/lib/notify";
import { toast } from "@/components/ui/toaster";
import { translate } from "@/lib/i18n";
import { logFrontendWarning } from "@/lib/error-log";

/**
 * `.catch` handlers that say what a failure means, instead of an anonymous `() => {}`:
 * - `ignore(why)`: expected and harmless here (a poll read again on its next tick, a listener
 *   that never got set up); `why` says so at the call site.
 * - `warn(what)`: shouldn't happen and nobody is waiting on it; logged so it isn't lost.
 * - `tell(title)`: the player asked for this and it didn't happen; tell them (an in-app toast
 *   while the window has focus, an OS notification otherwise), and log it.
 */
export const ignore = (why: string) => (e: unknown) => console.debug(`ignored (${why}):`, e);

export const warn = (what: string) => (e: unknown) => {
  console.warn(`${what}:`, e);
  logFrontendWarning(what, e);
};

export const tell = (title: string) => (e: unknown) => {
  warn(title)(e);
  if (typeof document !== "undefined" && document.hasFocus()) toast(title, String(e), "fail");
  else void notify(title, String(e));
};

/** Opens a link in the player's browser; when that fails, says so with the link to open by hand. */
export function openExternal(url: string) {
  openUrl(url).catch((e) => tell(translate("errors.browser"))(`${url} (${String(e)})`));
}
