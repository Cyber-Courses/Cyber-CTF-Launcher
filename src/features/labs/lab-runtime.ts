import { Cloud, Container, Globe, Monitor, Server, type LucideIcon } from "lucide-react";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabStatus, Provider, ServerHost, SystemReport } from "@/lib/tauri";

// Where a lab can run, from its runtime (catalogue) and what this machine has ready.

const SERVERS = new Set(["vmware_esxi", "proxmox"]);
export const CLOUDS = new Set(["aws", "azure", "gcp", "digitalocean", "linode", "oci"]);
// VM labs: one VM per machine on ESXi, Proxmox and AWS (Isoloom's vagrant, proxmox and cloud-vm
// outputs); the other clouds run container labs only for now.
const VM_CLOUDS_NOT_YET = new Set(["azure", "gcp", "digitalocean", "linode", "oci"]);

/** Hypervisors that run VMs built for another CPU, emulated (slowly): QEMU runs an x86 lab on
 *  an Apple Silicon Mac. */
export const EMULATORS: readonly Provider[] = ["qemu"];

// providers also carries "hosted" (not a launcher Provider), so compare as strings.
const isLocal = (p: string) => !SERVERS.has(p) && !CLOUDS.has(p) && p !== "hosted";

/** Whether the lab is built for this CPU (no architectures listed = any). */
export const runsNatively = (rt: LabRuntimeInfo, hostArch: string) => !rt.architectures.length || rt.architectures.includes(hostArch);

/** Whether an emulator (QEMU) is ready on this machine: Vagrant, the hypervisor and its plugin. */
export const emulatorReady = (report: SystemReport | null | undefined) =>
  !!report?.vagrant.installed && report.vmProviders.some((p) => EMULATORS.includes(p.provider) && !p.remote && p.available && p.hypervisor !== false);

/** The local hypervisors ready for a lab VM (Vagrant + hypervisor), `preferred` (Settings'
 *  choice) first. Only the report's fields this reads, so a partial report type fits. */
export function readyHypervisors(
  report: {
    vagrant: { installed: boolean };
    vmProviders: { provider: Provider; remote: boolean; available: boolean; hypervisor?: boolean | null }[];
  },
  preferred: Provider | null | undefined,
): Provider[] {
  const ready = report.vagrant.installed ? report.vmProviders.filter((p) => !p.remote && p.available && p.hypervisor !== false).map((p) => p.provider) : [];
  return preferred && ready.includes(preferred) ? [preferred, ...ready.filter((p) => p !== preferred)] : ready;
}

/** The hypervisors on this machine that can run the lab's VMs: those the lab lists, except for
 *  a VM lab built for another CPU, which only an emulator can run (whatever the lab lists: its
 *  list names the hypervisors of its own CPU). */
export function localProviders(rt: LabRuntimeInfo, hostArch?: string): Provider[] {
  // Cloud services run in the player's cloud account only.
  if (rt.runtime === "CLOUD") return [];
  if (rt.runtime === "VM" && hostArch && !runsNatively(rt, hostArch)) return [...EMULATORS];
  return rt.providers.filter((p: string) => isLocal(p));
}

/** The hypervisor a VM lab starts on here: a ready one among those it can use (`local`), else
 *  null. Never the lab's first listed provider: the catalogue lists them alphabetically, and
 *  picking e.g. "parallels" on a VirtualBox machine fails at once with "prlctl was not found".
 *  With no machine report yet (`ready` undefined), the Settings preference, then the first one. */
export function pickLocalHypervisor(local: Provider[], ready: Provider[] | undefined, preferred: Provider | null): Provider | null {
  if (ready === undefined) return (preferred && local.includes(preferred) ? preferred : local.at(0)) ?? null;
  return ready.find((p) => local.includes(p)) ?? null;
}

/** Whether a saved server or cloud account can run the lab. */
export const hostSupports = (rt: LabRuntimeInfo | null, h: ServerHost) =>
  !!rt?.providers.includes(h.provider) && !(rt.runtime === "VM" && VM_CLOUDS_NOT_YET.has(h.provider));

type PlaceKey = NonNullable<LabStatus["place"]> | "hosted";

/** Every place a lab could run, and whether this one can (its runtime here, then its providers).
 *  Its words are `labs.places.<key>`.
 *  A VM lab built for another CPU (x86 Windows on Apple Silicon) runs on this machine only
 *  emulated, so "VM on this machine" needs `emulates` (QEMU ready); containers run emulated
 *  anyway. */
export function runPlaces(rt: LabRuntimeInfo, hostArch?: string, emulates = false): { key: PlaceKey; icon: LucideIcon; available: boolean }[] {
  if (rt.runtime === "CLOUD") return cloudPlaces();
  const local = rt.providers.some((p: string) => isLocal(p));
  const vm = rt.runtime === "VM";
  return [
    { key: "container", icon: Container, available: !vm },
    { key: "local_vm", icon: Monitor, available: (vm || local) && (!(vm && hostArch && !runsNatively(rt, hostArch)) || emulates) },
    { key: "server", icon: Server, available: rt.providers.some((p) => SERVERS.has(p)) },
    { key: "cloud", icon: Cloud, available: rt.providers.some((p) => CLOUDS.has(p)) },
    { key: "hosted", icon: Globe, available: rt.hosted ?? false },
  ];
}

/** Cloud services: the player's cloud account, nowhere else. */
function cloudPlaces(): { key: PlaceKey; icon: LucideIcon; available: boolean }[] {
  return [
    { key: "container", icon: Container, available: false },
    { key: "local_vm", icon: Monitor, available: false },
    { key: "server", icon: Server, available: false },
    { key: "cloud", icon: Cloud, available: true },
    { key: "hosted", icon: Globe, available: false },
  ];
}

/** Whether the lab is cloud services (created in the player's cloud account, no machines). */
export const isCloudLab = (rt: LabRuntimeInfo | null | undefined) => rt?.runtime === "CLOUD";

/** The runtime column's word: what the lab is made of. */
export const runtimeLabel = (rt: LabRuntimeInfo) => (rt.runtime === "VM" ? "vm" : rt.runtime === "CLOUD" ? "cloud" : "docker");

const CLOUD_NAMES: Record<string, string> = {
  aws: "AWS",
  azure: "Azure",
  gcp: "Google Cloud",
  digitalocean: "DigitalOcean",
  linode: "Linode",
  oci: "Oracle Cloud",
};

/** The names of the clouds a cloud lab runs on ("AWS", "Google Cloud"), in its order. */
export const cloudNames = (providers: readonly string[]): string[] => providers.filter((p) => CLOUD_NAMES[p]).map((p) => CLOUD_NAMES[p]);

/** The player's accounts a cloud lab can run on: of a cloud it lists. */
export const cloudAccountsFor = (rt: LabRuntimeInfo | null | undefined, hosts: ServerHost[]) =>
  rt && isCloudLab(rt) ? hosts.filter((h) => CLOUDS.has(h.provider) && hostSupports(rt, h)) : [];
