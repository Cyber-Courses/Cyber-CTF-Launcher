import { confirmDestructive } from "@/lib/confirm";
import type { T } from "@/lib/i18n";

// Stop & remove deletes a lab's machines (or, on a cloud, its services): asked first, in the
// system's dialog, from the lab's page and from a list row alike. Removing a stopped lab from this
// computer (its files, not its machines: there are none by then) is asked the same way.

/** From the lab's page. `canShutdown`: says Shut down keeps them. */
export function confirmRemoveOnPage(t: T, { title, canShutdown, cloud }: { title: string; canShutdown: boolean; cloud: boolean }) {
  return confirmDestructive({
    title: t("labs.detail.removeTitle", { title }),
    message: t(cloud ? "labs.detail.removeBodyCloud" : "labs.detail.removeBody") + (canShutdown ? t("labs.detail.removeKeep") : ""),
    confirm: t("labs.detail.remove"),
  });
}

/** From a list row. `canShutdown`: says the lab's page can shut it down instead. */
export function confirmRemoveOnRow(t: T, { title, canShutdown, cloud }: { title: string; canShutdown: boolean; cloud: boolean }) {
  return confirmDestructive({
    title: t("labs.row.removeTitle", { title }),
    message: t(cloud ? "labs.row.removeBodyCloud" : "labs.row.removeBody") + (canShutdown ? t("labs.row.removeKeep") : ""),
    confirm: t("labs.row.remove"),
  });
}

/** Removing a stopped lab from this computer. `imported`: says the folder it came from is kept;
 *  a catalogue lab's says the account keeps its progress. */
export function confirmRemoveLab(t: T, { title, imported }: { title: string; imported: boolean }) {
  return confirmDestructive({
    title: t("labs.uninstall.confirmTitle", { title }),
    message: t(imported ? "labs.uninstall.confirmBodyImported" : "labs.uninstall.confirmBodyCatalogue"),
    confirm: t("labs.uninstall.confirm"),
  });
}
