import type { Dependency, ProviderStatus, SystemReport } from "@/lib/tauri";

export const PROVIDER_LABELS: Record<string, string> = {
  virtualbox: "VirtualBox",
  vmware_desktop: "VMware Workstation / Fusion",
  hyperv: "Hyper-V",
  parallels: "Parallels",
  libvirt: "libvirt (KVM)",
  qemu: "QEMU",
  utm: "UTM",
  vmware_esxi: "VMware ESXi (remote)",
  proxmox: "Proxmox VE (remote)",
};

/** A hypervisor's name on this OS: VMware's desktop hypervisor is Fusion on macOS and
 *  Workstation elsewhere, so a Linux or Windows player isn't pointed at a Mac-only product. */
export function providerLabel(p: ProviderStatus, os?: string): string {
  if (p.provider === "vmware_desktop" && os) return os === "macos" ? "VMware Fusion" : "VMware Workstation";
  return PROVIDER_LABELS[p.provider] ?? p.provider;
}

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
