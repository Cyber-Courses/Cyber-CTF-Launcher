import { openUrl } from "@tauri-apps/plugin-opener";
import { notify } from "@/lib/notify";
import { toast } from "@/components/ui/toaster";
import { translate } from "@/lib/i18n";
import { warn } from "@/lib/failure-log";
import { describeError } from "@/lib/errors";

/**
 * `.catch` handlers that say what a failure means, instead of an anonymous `() => {}`:
 * - `ignore(why)`: expected and harmless here (a poll read again on its next tick, a listener
 *   that never got set up); `why` says so at the call site.
 * - `warn(what)`: shouldn't happen and nobody is waiting on it; logged so it isn't lost.
 * - `tell(title)`: the player asked for this and it didn't happen; tell them (an in-app toast
 *   while the window has focus, an OS notification otherwise), and log it.
 *
 * `ignore` and `warn` take what to show `instead` (an empty list, signed out…): its value is the
 * handler's, so `.catch(ignore("…", () => null))` reads a failure as null. They live in
 * `@/lib/failure-log`, which depends on nothing translated, so the language's own storage can use
 * them; every other module takes them from here.
 */
export { ignore, warn } from "@/lib/failure-log";

export const tell = (title: string) => (e: unknown) => {
  warn(title)(e);
  const why = describeError(e);
  if (typeof document !== "undefined" && document.hasFocus()) toast(title, why, "fail");
  else void notify(title, why);
};

/** Opens a link in the player's browser; when that fails, says so with the link to open by hand. */
export function openExternal(url: string) {
  openUrl(url).catch((e: unknown) => {
    tell(translate("errors.browser"))(`${url} (${describeError(e)})`);
  });
}
