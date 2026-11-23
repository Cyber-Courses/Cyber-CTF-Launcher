import type { ProxmoxNodeChoices, ProxmoxPlacement } from "@/lib/tauri";

/** A one-node home Proxmox server as its API lists it: two storages, its LAN bridge (`vmbr1`,
 *  the default) and an internal one. The preview, the test stub and the setup tests share it. */
export const PROXMOX_NODE: ProxmoxNodeChoices = {
  name: "pve",
  diskStorages: ["local", "local-lvm"],
  imageStorages: ["local"],
  snippetStorages: ["local"],
  bridges: [
    { name: "vmbr0", cidr: "192.168.150.10/24" },
    { name: "vmbr1", cidr: "10.0.0.50/24" },
  ],
  defaultDisk: "local-lvm",
  defaultImage: "local",
  defaultSnippet: "local",
  defaultBridge: "vmbr1",
};

export const PROXMOX_PLACEMENT: ProxmoxPlacement = { nodes: [PROXMOX_NODE] };
