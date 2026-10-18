// Servers and cloud accounts saved in the browser preview, shared by its tabs (the setup window
// saves, the main window lists): kept in the browser's storage.
import { answer } from "@/lib/dev-mock/sample";
import { argText } from "@/lib/dev-mock/command";
import type { Scenario } from "@/lib/dev-mock/scenario";

/** A saved server or cloud account, as the setup sends it and the list answers it. */
export type Host = Record<string, unknown> & { id?: string | null };

const HOSTS_KEY = "cyberctf.devMock.hosts";

/** The hosts saved in the preview; null until one is saved. */
function savedHosts(): Host[] | null {
  try {
    const raw = localStorage.getItem(HOSTS_KEY);
    return raw ? (JSON.parse(raw) as Host[]) : null;
  } catch {
    return null;
  }
}

export function saveHosts(hosts: Host[]) {
  try {
    localStorage.setItem(HOSTS_KEY, JSON.stringify(hosts));
  } catch {
    /* storage unavailable: the save is not kept */
  }
}

/** A few of the core's checks on a saved server (runtime/server/input.rs), with its messages. */
export function saveError(h: Host): string | null {
  const host = argText(h.host).trim();
  const user = argText(h.username).trim();
  if (h.provider === "proxmox" || h.provider === "vmware_esxi") {
    if (!/^[A-Za-z0-9.-]+$/.test(host)) return "host must be a hostname or IP address, without https:// or a path";
    if (h.provider === "proxmox" && !user.includes("@")) return "Proxmox users include a realm, e.g. root@pam";
  }
  if (h.provider === "aws" && !h.useCliCreds && !/^AKIA[A-Z0-9]{16}$/.test(user)) return "access key id looks wrong; an IAM user's key starts with AKIA...";
  return null;
}

/** The saved servers and cloud accounts: those saved in the preview, else the sample ones (without
 *  the cloud account under `cloud=none`). */
export function hostsOf(sc: Scenario): Host[] {
  const sample = (answer("server_list", undefined) as { hosts: Host[] }).hosts;
  return savedHosts() ?? (sc.noCloud ? sample.filter((h) => h.provider !== "aws") : sample);
}
