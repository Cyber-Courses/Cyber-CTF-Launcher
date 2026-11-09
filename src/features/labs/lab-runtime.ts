import { Cloud, Container, Monitor, Server, type LucideIcon } from "lucide-react";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { EMULATORS, LOCAL_PROVIDERS, PROVIDER_NAMES, type LabRunOptions, type LabStatus, type Provider, type ServerHost } from "@/lib/tauri";
import { CLOUD_PROVIDERS } from "@/features/cloud/cloud-model";
import type { RunTarget } from "@/features/labs/run-on";

// Where a lab can run, from its runtime (catalogue) and what this machine has ready.

const SERVERS = new Set(["vmware_esxi", "proxmox"]);
export const CLOUDS = new Set(CLOUD_PROVIDERS);

/** Whether `p` is a hypervisor a lab's VMs run on here (the provider a local VM lab's status
 *  names): one of the backend's local providers. */
export const isLocalHypervisor = (p: string | null | undefined): p is Provider => !!p && (LOCAL_PROVIDERS as readonly string[]).includes(p);

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

/** The hypervisors a form of the lab runs on here, as the backend's run options say (`paths`:
 *  the helper VM's, or one VM per machine's); for VMs built for another CPU, the emulator only. */
export function formProviders(paths: LabRunOptions["vms"], rt: LabRuntimeInfo, hostArch?: string): Provider[] {
  const providers = paths.flatMap((p) => (p.provider ? [p.provider] : []));
  return hostArch && !runsNatively(rt, hostArch) ? providers.filter((p) => EMULATORS.includes(p)) : providers;
}

/** Whether an emulator (QEMU) is among the hypervisors `ready` here (the machine report's). */
export const emulatorReady = (ready: readonly Provider[]) => ready.some((p) => EMULATORS.includes(p));

/** The hypervisors ready here (the machine report's `readyHypervisors`), `preferred` (Settings'
 *  choice) first: the picker's default for a container lab's VM forms. */
export const preferredFirst = (ready: readonly Provider[], preferred: Provider | null): Provider[] =>
  preferred && ready.includes(preferred) ? [preferred, ...ready.filter((p) => p !== preferred)] : [...ready];

/** The hypervisors on this machine that can run a VM lab's VMs: as the backend's run options say
 *  (`options`, once read), else those the catalogue lists; a VM lab built for another CPU only
 *  emulated. A container lab's helper VM is the run options' too (`formProviders`), and none for
 *  cloud services. */
export function localProviders(rt: LabRuntimeInfo, hostArch?: string, options?: LabRunOptions | null): Provider[] {
  if (rt.runtime !== "VM") return options ? formProviders(options.dockerVm, rt) : [];
  const listed = options ? formProviders(options.vms, rt) : rt.providers.filter(isLocalHypervisor);
  // A VM edition built for another CPU runs here only emulated (whatever hypervisors it lists).
  return hostArch && !runsNatively(rt, hostArch) ? [...EMULATORS] : listed;
}

/** Whether a saved server or cloud account can run the lab: as the backend's run options say
 *  (`options`: a path on its kind of server or its cloud), else, until they are read, when the
 *  catalogue lists its provider. */
export function hostSupports(rt: LabRuntimeInfo | null, h: ServerHost, options?: LabRunOptions | null): boolean {
  if (!options) return !!rt?.providers.includes(h.provider);
  const paths = CLOUDS.has(h.provider) ? [...options.cloudDocker, ...options.cloudVm, ...options.cloudServices] : options.servers;
  return paths.some((p) => p.provider === h.provider);
}

type PlaceKey = NonNullable<LabStatus["place"]>;

/** A place's state: runnable now (`ready`), supported by the lab but not set up in the launcher
 *  yet (`configurable` — install a hypervisor, connect a server or a cloud account), or not a
 *  thing this lab can run as at all (`unsupported`). */
export type PlaceState = "ready" | "configurable" | "unsupported";

/** What the launcher is set up with, so a place the lab supports but the launcher can't reach yet
 *  reads as `configurable`, not `ready`: the local hypervisors ready here, and the saved servers
 *  and cloud accounts. */
export type LauncherSetup = { readyVms?: Provider[]; hosts?: ServerHost[]; options?: LabRunOptions | null };

const placeState = (supported: boolean, configured: boolean): PlaceState => (supported ? (configured ? "ready" : "configurable") : "unsupported");

const cloudConfigured = (rt: LabRuntimeInfo, hosts: ServerHost[], options: LabRunOptions | null) =>
  hosts.some((h) => CLOUDS.has(h.provider) && hostSupports(rt, h, options));

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
  const { readyVms = [], hosts = [], options = null } = setup;
  const w = ways(rt);
  // Cloud services (no container or VM edition) run in the player's cloud account only.
  if (w.cloud && !w.docker && !w.vm) return cloudPlaces(rt, hosts, options);
  // A container lab runs in a helper VM here where the backend's run options say, else (until
  // they are read) wherever the catalogue lists no server or cloud only.
  const local = options ? options.dockerVm.length > 0 : rt.providers.some((p) => !SERVERS.has(p) && !CLOUDS.has(p));
  // The VM edition built for another CPU runs here only emulated (containers run emulated anyway).
  const vmForeignCpu = w.vm && !!hostArch && !runsNatively(rt, hostArch);
  const localVmSupported = (w.vm || (w.docker && local)) && (!vmForeignCpu || emulates);
  const localVmReady = readyVms.some((p) => localProviders(rt, hostArch, options).includes(p));
  const serverSupported = (w.vm || w.docker) && rt.providers.some((p) => SERVERS.has(p));
  const serverReady = hosts.some((h) => SERVERS.has(h.provider) && hostSupports(rt, h, options));
  return [
    { key: "container", icon: Container, state: placeState(w.docker, true) },
    { key: "local_vm", icon: Monitor, state: placeState(localVmSupported, localVmReady) },
    { key: "server", icon: Server, state: placeState(serverSupported, serverReady) },
    {
      key: "cloud",
      icon: Cloud,
      state: placeState(
        rt.providers.some((p) => CLOUDS.has(p)),
        cloudConfigured(rt, hosts, options),
      ),
    },
  ];
}

/** Cloud services: the player's cloud account, nowhere else (configurable until one is connected). */
function cloudPlaces(rt: LabRuntimeInfo, hosts: ServerHost[], options: LabRunOptions | null): { key: PlaceKey; icon: LucideIcon; state: PlaceState }[] {
  return [
    { key: "container", icon: Container, state: "unsupported" },
    { key: "local_vm", icon: Monitor, state: "unsupported" },
    { key: "server", icon: Server, state: "unsupported" },
    { key: "cloud", icon: Cloud, state: placeState(true, cloudConfigured(rt, hosts, options)) },
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

/** The names of the clouds a cloud lab runs on ("AWS", "Google Cloud"), in its order. */
export const cloudNames = (providers: readonly string[]): string[] => providers.filter((p): p is Provider => CLOUDS.has(p)).map((p) => PROVIDER_NAMES[p]);

/** The player's accounts a cloud lab can run on: of a cloud it lists. */
export const cloudAccountsFor = (rt: LabRuntimeInfo | null | undefined, hosts: ServerHost[], options?: LabRunOptions | null) =>
  rt && isCloudLab(rt) ? hosts.filter((h) => CLOUDS.has(h.provider) && hostSupports(rt, h, options)) : [];

/** A container lab's VM form here (`local-vm`: one helper VM, `local-vms`: one VM per machine) on
 *  hypervisor `provider`. */
export const vmFormTarget = (form: "local-vm" | "local-vms", provider: Provider): RunTarget =>
  form === "local-vm" ? { kind: form, provider } : { kind: form, provider };
