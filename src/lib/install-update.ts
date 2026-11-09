import type { Update } from "@tauri-apps/plugin-updater";
import { deployingLabs } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { toast } from "@/components/ui/toaster";
import { translate } from "@/lib/i18n";

/**
 * Downloads and installs `update`, the About section's and the banner's one install. Installing
 * quits every Cyber CTF process, the deploy workers included, so it waits while a lab deploys
 * (asked again now: the polled set can be a few seconds old), and also when that can't be told.
 * Each way it doesn't install is told; `starting` runs once the download begins. True once the
 * update is installed.
 */
export async function installUpdate(update: Update, starting: () => void): Promise<boolean> {
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
