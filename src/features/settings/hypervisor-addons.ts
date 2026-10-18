"use client";

import { useState } from "react";
import { hypervJoinAdmins, installVagrantPlugin, type ProviderStatus } from "@/lib/tauri";
import { translate } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

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
  const [log, setLog] = useState<string[] | null>(null);

  async function install(plugin: string) {
    setInstalling(plugin);
    setLog([translate("settings.hypervisor.logInstalling", { plugin })]);
    try {
      await installVagrantPlugin(plugin, (l) => {
        setLog((x) => [...(x ?? []), l]);
      });
      setLog((x) => [...(x ?? []), translate("settings.hypervisor.logInstalled")]);
    } catch (e) {
      setLog((x) => [...(x ?? []), `✗ ${describeError(e)}`]);
    } finally {
      setInstalling(null);
      onRefresh();
    }
  }

  async function allow() {
    setInstalling(HYPERV_RIGHTS);
    setLog([translate("settings.hypervisor.logAllowingHyperv")]);
    try {
      await hypervJoinAdmins();
      setLog((x) => [...(x ?? []), translate("settings.hypervisor.logHypervAllowed")]);
    } catch (e) {
      setLog((x) => [...(x ?? []), `✗ ${describeError(e)}`]);
    } finally {
      setInstalling(null);
      onRefresh();
    }
  }

  // Click handlers: each says how it went in the log.
  const installAddon = (plugin: string) => {
    void install(plugin);
  };
  const allowHyperv = () => {
    void allow();
  };
  return { installing, log, installAddon, allowHyperv };
}
