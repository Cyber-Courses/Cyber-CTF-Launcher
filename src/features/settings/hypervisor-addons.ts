"use client";

import { useState } from "react";
import { hypervJoinAdmins, installVagrantPlugin, type ProviderStatus } from "@/lib/tauri";
import { useRunLog } from "@/features/servers/host-setup/use-run-log";
import { translate } from "@/lib/i18n";

// What a hypervisor that is installed but can't run VM labs yet needs: its Vagrant add-on, or
// (Hyper-V) this account's right to manage it.

/** What `installing` holds while this account is being allowed to manage Hyper-V. */
export const HYPERV_RIGHTS = "hyperv-rights";

/** Hyper-V is on and Vagrant installed, yet not ready: the one thing left is this account's
 *  right to manage Hyper-V (Vagrant needs an administrator or a Hyper-V Administrators member). */
export const needsHypervRights = (h: ProviderStatus) => h.provider === "hyperv" && h.hypervisor === true && !h.available;

/** Installs a hypervisor's Vagrant add-on (or allows this account to manage Hyper-V), with its
 *  log, then re-checks the machine. */
export function useAddonInstall(onRefresh: () => void) {
  const [installing, setInstalling] = useState<string | null>(null);
  const { lines, run } = useRunLog();

  // Install an add-on or claim the Hyper-V right: one run with its log, differing only in what it
  // marks busy, the task it awaits, and the lines around it.
  async function perform(key: string, first: string, task: (onLog: (l: string) => void) => Promise<string>) {
    setInstalling(key);
    await run(first, task);
    setInstalling(null);
    onRefresh();
  }

  // Click handlers: each says how it went in the log.
  const installAddon = (plugin: string) => {
    void perform(plugin, translate("settings.hypervisor.logInstalling", { plugin }), async (onLog) => {
      await installVagrantPlugin(plugin, onLog);
      return translate("settings.hypervisor.logInstalled");
    });
  };
  const allowHyperv = () => {
    void perform(HYPERV_RIGHTS, translate("settings.hypervisor.logAllowingHyperv"), async () => {
      await hypervJoinAdmins();
      return translate("settings.hypervisor.logHypervAllowed");
    });
  };
  return { installing, log: lines, installAddon, allowHyperv };
}
