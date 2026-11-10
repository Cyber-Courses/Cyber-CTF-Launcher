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

/** The hypervisors offered on this machine: the backend lists only those for its OS and CPU (UTM,
 *  not VirtualBox, on Apple Silicon; one already installed kept), so these are its local ones. */
export function localHypervisors(report: SystemReport): ProviderStatus[] {
  return report.vmProviders.filter((p) => !p.remote);
}

/** Whether Vagrant can drive `p` here: nothing left to install for it (Vagrant, its plugin, and
 *  for VMware HashiCorp's utility), as the backend lists it (`needs`, the same list its readiness
 *  reads). The one rule the Machine page's rows, the setup steps and their "done" all read, so
 *  finishing the steps can't leave a hypervisor "needs setup". */
export const vagrantDrives = (p: ProviderStatus): boolean => p.needs.length === 0;

/** An installed hypervisor that still can't run labs once everything Vagrant needs is in place
 *  (no /dev/kvm, not in the kvm group, a stuck service): nothing later in setup fixes it, and
 *  `p.reason` says why. Something still to install isn't this; setup installs it next. */
export function cantRun(p: ProviderStatus): boolean {
  return p.hypervisor === true && !p.available && !!p.reason && vagrantDrives(p);
}
