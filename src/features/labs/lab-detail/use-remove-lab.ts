"use client";

import { useEffect, useState } from "react";
import { labInstalled, labRemove, type LabRemoval } from "@/lib/tauri";
import { confirmRemoveLab } from "@/features/labs/confirm-remove";
import { refreshImported } from "@/features/labs/imported-labs";
import { refreshStatus } from "@/features/labs/lab-store";
import { forgetLastRun } from "@/features/labs/runs/last-run";
import type { Lab } from "@/features/labs/use-labs";
import { warnInDialog } from "@/lib/confirm";
import { ignore, tell } from "@/lib/failure";
import { toast } from "@/lib/toast";
import { useT, type T } from "@/lib/i18n";

// Removing a lab from this computer, from its page: the app's copy of it and what the app keeps
// for it (the core's `lab_remove`), once the player confirmed it, never while it is up.

/** Whether `lab` is installed here (has anything to remove): an imported lab always is (only an
 *  installed one is listed); a catalogue lab's is read again whenever `changed` does (its state:
 *  a start installs it). Null until known. */
function useInstalled(lab: Lab, changed: string): boolean | null {
  const [read, setRead] = useState<{ id: string; installed: boolean } | null>(null);
  useEffect(() => {
    if (lab.imported) return;
    let live = true;
    labInstalled(lab.id)
      .then((installed) => {
        if (live) setRead({ id: lab.id, installed });
      })
      .catch(ignore("read again when the lab's state changes"));
    return () => {
      live = false;
    };
  }, [lab.id, lab.imported, changed]);
  if (lab.imported) return true;
  return read?.id === lab.id ? read.installed : null;
}

/** What the removal says once done: removed (a toast), or removed with files left behind (the
 *  system dialog, which stays until read, naming each one). */
function sayRemoved(t: T, lab: Lab, { left }: LabRemoval) {
  if (!left.length) {
    toast(t("labs.uninstall.removed", { title: lab.title }), lab.imported ? t("labs.uninstall.removedImported") : undefined);
    return Promise.resolve();
  }
  const paths = left.map((l) => `${l.path} (${l.error})`).join("\n");
  return warnInDialog({ title: t("labs.uninstall.partialTitle", { title: lab.title }), message: t("labs.uninstall.partialBody", { paths }) });
}

/**
 * The page's "Remove lab": whether to offer it (the lab is installed), and the removal, which asks
 * first and, once done, tells the player and goes back to the list (`onRemoved`).
 */
export function useRemoveLab(lab: Lab, changed: string, onRemoved: () => void) {
  const t = useT();
  const installed = useInstalled(lab, changed);
  const [removing, setRemoving] = useState(false);
  const remove = async () => {
    if (!(await confirmRemoveLab(t, { title: lab.title, imported: !!lab.imported }))) return;
    setRemoving(true);
    try {
      const removal = await labRemove(lab.id);
      forgetLastRun(lab.id);
      // Its row goes from the lists, its status reads as not installed.
      void (lab.imported ? refreshImported() : refreshStatus(lab, { fresh: true }));
      await sayRemoved(t, lab, removal);
      onRemoved();
    } catch (e) {
      tell(t("labs.uninstall.failed"))(e);
      setRemoving(false);
    }
  };
  return { installed: installed === true, removing, remove };
}
