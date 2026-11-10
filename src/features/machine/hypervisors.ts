import { PROVIDER_NAMES, PROVIDER_NAMES_ON, type Provider, type ProviderStatus, type SystemReport } from "@/lib/tauri";
import { blockedReason } from "@/features/machine/setup-steps/plan";

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
 *  `p.reason` says why. Something still to install isn't this; setup installs it next. The core's
 *  plan says so (a `blocked` action). */
export const cantRun = (report: SystemReport, p: ProviderStatus): boolean => blockedReason(report, p.provider) !== null;
