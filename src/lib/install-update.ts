import type { Update } from "@tauri-apps/plugin-updater";
import { busyLabs, restartApp } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { toast } from "@/lib/toast";
import { translate } from "@/lib/i18n";

/**
 * Downloads and installs `update`, the About section's and the banner's one install, then reopens
 * the app on it: `starting` runs once the download begins, `installed` once it is installed,
 * before the restart. False when it wasn't installed (each way it doesn't is told, see
 * [`install`]); a restart that fails after a good install is told too (the update is installed:
 * Restart now stays for another try, see [`restartOnUpdate`]).
 */
export function installAndRestart(update: Update, starting: () => void, installed: () => void): Promise<boolean> {
  // One at a time: the button stays enabled while the running labs are asked, and a double
  // click downloaded and installed the update twice at once. A second ask joins the first.
  inFlight ??= installThenRestart(update, starting, installed).finally(() => {
    inFlight = null;
  });
  return inFlight;
}

/** The install under way in this window, if one is. */
let inFlight: Promise<boolean> | null = null;

async function installThenRestart(update: Update, starting: () => void, installed: () => void): Promise<boolean> {
  if (!(await install(update, starting))) return false;
  installed();
  await restartOnUpdate();
  return true;
}

/** Reopens the app on the update just installed; a failure is told, with one message wherever it
 *  is asked from (after an install, or Restart now in the banner or Settings). */
export function restartOnUpdate(): Promise<void> {
  return restartApp().catch(tell(translate("shell.failures.restart")));
}

/**
 * Downloads and installs `update`. Installing quits every Cyber CTF process, the runner service
 * included, so it waits while any lab operation runs, a stop or a pause as much as a start
 * (`busyLabs`, asked again now: the operations the window holds can lag a moment behind), and also when that
 * can't be told. Each way it doesn't install is told; `starting` runs once the download begins.
 * True once the update is installed.
 */
async function install(update: Update, starting: () => void): Promise<boolean> {
  let busy: string[];
  try {
    busy = await busyLabs();
  } catch (e) {
    tell(translate("shell.update.busyUnknown"))(e);
    return false;
  }
  if (busy.length > 0) {
    toast(translate("shell.update.waitForLabs"), undefined, "warn");
    return false;
  }
  starting();
  try {
    await update.downloadAndInstall();
  } catch (e) {
    tell(translate("shell.update.failed"))(e);
    return false;
  }
  return true;
}
