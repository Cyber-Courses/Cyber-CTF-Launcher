import type { Provider } from "@/lib/tauri";

/** What a provider's avatar shows where the launcher ships no logo for it (no VMware, Microsoft or
 *  Parallels mark may be bundled; libvirt has none): the same letters on every page. */
const INITIALS: Partial<Record<Provider, string>> = { vmware_esxi: "ES", hyperv: "HV", parallels: "PD", libvirt: "LV" };

/** A provider's initials, if it has any (one without a logo); VMware's desktop hypervisor is Fusion
 *  on macOS ("VF") and Workstation elsewhere ("VW"), as its name on `os`. */
export function providerInitials(provider: Provider, os?: string): string | undefined {
  if (provider === "vmware_desktop") return os === "macos" ? "VF" : "VW";
  return INITIALS[provider];
}
