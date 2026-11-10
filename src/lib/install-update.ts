import type { Update } from "@tauri-apps/plugin-updater";
import { deployingLabs, restartApp } from "@/lib/tauri";
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
export async function installAndRestart(update: Update, starting: () => void, installed: () => void): Promise<boolean> {
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
 * Downloads and installs `update`. Installing
 * quits every Cyber CTF process, the deploy workers included, so it waits while a lab deploys
 * (asked again now: the polled set can be a few seconds old), and also when that can't be told.
 * Each way it doesn't install is told; `starting` runs once the download begins. True once the
 * update is installed.
 */
async function install(update: Update, starting: () => void): Promise<boolean> {
  let deploying: string[];
  try {
    deploying = await deployingLabs();
  } catch (e) {
    tell(translate("shell.update.deployingUnknown"))(e);
    return false;
  }
  if (deploying.length > 0) {
    toast(translate("shell.update.waitForDeploys"), undefined, "warn");
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
