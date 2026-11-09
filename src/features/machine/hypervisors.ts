import { PROVIDER_NAMES, PROVIDER_NAMES_ON, type Dependency, type Provider, type ProviderStatus, type SystemReport } from "@/lib/tauri";

/** A provider's name, by its id (a status's, a workload's), from the backend's names: on the OS
 *  `os` (the machine report's) when it is known, where an OS changes some (VMware's desktop
 *  hypervisor is Fusion on macOS, Workstation elsewhere). An id the backend doesn't name reads as
 *  itself. */
export function providerName(id: string, os?: string): string {
  const names: Partial<Record<string, string>> = PROVIDER_NAMES;
  const onOs: Partial<Record<string, Partial<Record<Provider, string>>>> = PROVIDER_NAMES_ON;
  return (os ? onOs[os]?.[id as Provider] : undefined) ?? names[id] ?? id;
}

/** A hypervisor's name on this OS (`os`, the machine report's): see [`providerName`]. */
export const providerLabel = (p: ProviderStatus, os?: string): string => providerName(p.provider, os);

/** Hypervisors we can install in one click (per-OS plans live in the Rust installer). */
export const INSTALLABLE: Partial<Record<string, Dependency>> = {
  virtualbox: "virtualbox",
  qemu: "qemu",
  utm: "utm",
  libvirt: "libvirt",
};

/** The rest are behind a login / paywall (or an OS feature), so we link to them. */
export const DOWNLOAD: Partial<Record<string, string>> = {
  vmware_desktop: "https://www.vmware.com/products/desktop-hypervisor/workstation-and-fusion",
  parallels: "https://www.parallels.com/products/desktop/",
  hyperv: "https://learn.microsoft.com/virtualization/hyper-v-on-windows/quick-start/enable-hyper-v",
};

/** Local hypervisors usable on this OS + arch (always keeps ones already installed).
 *  VirtualBox has no Apple-Silicon support; UTM is for Apple Silicon. */
export function usableHypervisors(report: SystemReport): ProviderStatus[] {
  const isMac = report.os === "macos";
  const isArm = report.arch === "aarch64" || report.arch === "arm64";
  return report.vmProviders.filter((p) => {
    if (p.remote) return false;
    if (p.hypervisor === true) return true;
    if (p.provider === "virtualbox" && isMac && isArm) return false;
    if (p.provider === "utm" && isMac && !isArm) return false;
    return true;
  });
}

/** An installed hypervisor that still can't run labs once Vagrant and its plugin are in place
 *  (no /dev/kvm, not in the kvm group, a stuck service): nothing later in setup fixes it, and
 *  `p.reason` says why. Missing Vagrant alone isn't this; setup installs it next. */
export function cantRun(p: ProviderStatus, report: SystemReport): boolean {
  return p.hypervisor === true && !p.available && !!p.reason && report.vagrant.installed && (!p.plugin || p.pluginInstalled);
}
