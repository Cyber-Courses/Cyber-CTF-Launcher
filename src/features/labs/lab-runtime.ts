import { Cloud, Container, Monitor, Server, type LucideIcon } from "lucide-react";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabStatus, Provider, ServerHost, SystemReport } from "@/lib/tauri";
import { CLOUD_PROVIDERS } from "@/features/cloud/cloud-model";

// Where a lab can run, from its runtime (catalogue) and what this machine has ready.

const SERVERS = new Set(["vmware_esxi", "proxmox"]);
export const CLOUDS = new Set(CLOUD_PROVIDERS);

/** Hypervisors that run VMs built for another CPU, emulated (slowly): QEMU runs an x86 lab on
 *  an Apple Silicon Mac. */
export const EMULATORS: readonly Provider[] = ["qemu"];

/** The local hypervisors a container lab can run on as docker-in-VM (inside a generic helper VM),
 *  independent of the lab's own (native-VM) provider list. Hyper-V is left out: Isoloom's
 *  docker-vm manifest lists no Hyper-V provider (it needs elevation and a named switch), so the
 *  runner refuses the start with "provider hyperv isn't one of this run's providers" — this
 *  list mirrors that manifest, which the backend doesn't send. */
const LOCAL_HYPERVISORS: readonly Provider[] = ["virtualbox", "vmware_desktop", "parallels", "libvirt", "qemu", "utm"];

/** The local hypervisors Isoloom's per-machine VM form (its vagrant target) runs on: every one a
 *  container lab's helper VM does, and Hyper-V, which that form has (it asks for elevation). */
const VM_HYPERVISORS: readonly Provider[] = [...LOCAL_HYPERVISORS, "hyperv"];

/** Whether `p` is a hypervisor a lab's VMs run on here (the provider a local VM lab's status
 *  names). */
export const isLocalHypervisor = (p: string | null | undefined): p is Provider => !!p && (VM_HYPERVISORS as readonly string[]).includes(p);

// providers may still carry "hosted" (not a launcher Provider and no longer offered), so keep it
// out of the local hypervisors; compare as strings.
const isLocal = (p: string) => !SERVERS.has(p) && !CLOUDS.has(p) && p !== "hosted";

/** Whether the lab is built for this CPU (no architectures listed = any). */
export const runsNatively = (rt: LabRuntimeInfo, hostArch: string) => !rt.architectures.length || rt.architectures.includes(hostArch);

/** The ways a lab can run: the backend's capability flags when present, else derived from the
 *  single runtime kind so this is correct before a backend that records them. A dual-form lab has
 *  more than one true (e.g. docker and vm), and the launcher offers a place for each. */
const ways = (rt: LabRuntimeInfo) => ({
  docker: rt.docker ?? rt.runtime === "DOCKER",
  vm: rt.vm ?? rt.runtime === "VM",
  cloud: rt.cloud ?? rt.runtime === "CLOUD",
});

/** Whether the lab has both forms: a container recipe and a VM recipe per machine (the backend's
 *  `docker` and `vm` flags). Such a lab can run here as containers, as containers in one helper VM
 *  (docker-vm), or as one VM per machine like a VM lab (Isoloom's vagrant target). */
export const hasBothForms = (rt: LabRuntimeInfo) => {
  const w = ways(rt);
  return w.docker && w.vm;
};

/** The local hypervisors Isoloom refuses the per-machine VM form on: UTM's Vagrant plugin gives
 *  a VM no private network, so a lab's machines couldn't reach each other. Mirrors the backend's
 *  refusal at launch (`launch/local.rs::check_provider`, Isoloom's `vagrant_refusal`), which
 *  stays the rule: this only keeps the picker from offering what it would refuse. */
const NO_PER_MACHINE_VM: readonly Provider[] = ["utm"];

/** The hypervisors on this machine that can run a dual-form lab as one VM per machine: every
 *  local one but those Isoloom refuses the form on (`NO_PER_MACHINE_VM`); for a lab whose VMs
 *  are built for another CPU, the emulator only. Empty for a lab with a single form. */
export function perMachineProviders(rt: LabRuntimeInfo, hostArch?: string): Provider[] {
  if (!hasBothForms(rt)) return [];
  if (hostArch && !runsNatively(rt, hostArch)) return EMULATORS.filter((p) => !NO_PER_MACHINE_VM.includes(p));
  return VM_HYPERVISORS.filter((p) => !NO_PER_MACHINE_VM.includes(p));
}

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
  const w = ways(rt);
  // Cloud services (no container or VM edition) run in the player's cloud account only.
  if (w.cloud && !w.docker && !w.vm) return [];
  // A VM edition built for another CPU runs here only emulated (whatever hypervisors it lists).
  if (w.vm && hostArch && !runsNatively(rt, hostArch)) return [...EMULATORS];
  // A container lab runs as docker-in-VM inside a generic helper VM, so any local hypervisor can
  // host it, not only the lab's own (native-VM) providers.
  if (w.docker) return [...LOCAL_HYPERVISORS];
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

/** Whether a cloud account offers the "one VM per machine" form (Isoloom's cloud-vm) for this lab,
 *  as well as "one VM" (cloud-docker): a dual-form lab, on any cloud (each has a cloud-vm module;
 *  how far each is proven is its run tier's badge). A single-form lab offers one form, so the
 *  picker skips the step. */
export const cloudRunsPerMachine = (rt: LabRuntimeInfo | null, provider: string) => !!rt && hasBothForms(rt) && CLOUDS.has(provider);

/** Whether a saved server or cloud account can run the lab: the lab lists its provider. */
export const hostSupports = (rt: LabRuntimeInfo | null, h: ServerHost) => !!rt?.providers.includes(h.provider);

type PlaceKey = NonNullable<LabStatus["place"]>;

/** A place's state: runnable now (`ready`), supported by the lab but not set up in the launcher
 *  yet (`configurable` — install a hypervisor, connect a server or a cloud account), or not a
 *  thing this lab can run as at all (`unsupported`). */
export type PlaceState = "ready" | "configurable" | "unsupported";

/** What the launcher is set up with, so a place the lab supports but the launcher can't reach yet
 *  reads as `configurable`, not `ready`: the local hypervisors ready here, and the saved servers
 *  and cloud accounts. */
export type LauncherSetup = { readyVms?: Provider[]; hosts?: ServerHost[] };

const placeState = (supported: boolean, configured: boolean): PlaceState => (supported ? (configured ? "ready" : "configurable") : "unsupported");

const cloudConfigured = (rt: LabRuntimeInfo, hosts: ServerHost[]) => hosts.some((h) => CLOUDS.has(h.provider) && hostSupports(rt, h));

/** Every place a lab could run, and its state (see [`PlaceState`]). Its words are
 *  `labs.places.<key>`. `setup` is what the launcher has ready, used to tell a place that can run
 *  now from one the lab supports but the launcher isn't configured for.
 *  A VM lab built for another CPU (x86 Windows on Apple Silicon) runs on this machine only
 *  emulated, so "VM on this machine" needs `emulates` (QEMU ready); containers run emulated
 *  anyway. */
export function runPlaces(
  rt: LabRuntimeInfo,
  hostArch?: string,
  emulates = false,
  setup: LauncherSetup = {},
): { key: PlaceKey; icon: LucideIcon; state: PlaceState }[] {
  const { readyVms = [], hosts = [] } = setup;
  const w = ways(rt);
  // Cloud services (no container or VM edition) run in the player's cloud account only.
  if (w.cloud && !w.docker && !w.vm) return cloudPlaces(rt, hosts);
  const local = rt.providers.some((p: string) => isLocal(p));
  // The VM edition built for another CPU runs here only emulated (containers run emulated anyway).
  const vmForeignCpu = w.vm && !!hostArch && !runsNatively(rt, hostArch);
  const localVmSupported = (w.vm || (w.docker && local)) && (!vmForeignCpu || emulates);
  const localVmReady = readyVms.some((p) => localProviders(rt, hostArch).includes(p));
  const serverSupported = (w.vm || w.docker) && rt.providers.some((p) => SERVERS.has(p));
  const serverReady = hosts.some((h) => SERVERS.has(h.provider) && hostSupports(rt, h));
  return [
    { key: "container", icon: Container, state: placeState(w.docker, true) },
    { key: "local_vm", icon: Monitor, state: placeState(localVmSupported, localVmReady) },
    { key: "server", icon: Server, state: placeState(serverSupported, serverReady) },
    {
      key: "cloud",
      icon: Cloud,
      state: placeState(
        rt.providers.some((p) => CLOUDS.has(p)),
        cloudConfigured(rt, hosts),
      ),
    },
  ];
}

/** Cloud services: the player's cloud account, nowhere else (configurable until one is connected). */
function cloudPlaces(rt: LabRuntimeInfo, hosts: ServerHost[]): { key: PlaceKey; icon: LucideIcon; state: PlaceState }[] {
  return [
    { key: "container", icon: Container, state: "unsupported" },
    { key: "local_vm", icon: Monitor, state: "unsupported" },
    { key: "server", icon: Server, state: "unsupported" },
    { key: "cloud", icon: Cloud, state: placeState(true, cloudConfigured(rt, hosts)) },
  ];
}

/** Whether the lab is cloud services (created in the player's cloud account, no machines). */
export const isCloudLab = (rt: LabRuntimeInfo | null | undefined) => rt?.runtime === "CLOUD";

/** Whether the catalogue publishes the lab as containers (a lab not read yet counts as one). Where
 *  and in which form it actually runs is its status's (`place`, `perMachine`): this only decides
 *  what the launch picker offers and how the lab is named. */
export const isContainerLab = (rt: LabRuntimeInfo | null | undefined) => rt?.runtime !== "VM" && !isCloudLab(rt);

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
