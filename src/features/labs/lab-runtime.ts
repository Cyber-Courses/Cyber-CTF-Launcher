import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import {
  EMULATORS,
  LOCAL_PROVIDERS,
  PROVIDER_NAMES,
  SERVER_PROVIDERS,
  isCloudProvider,
  type LabRunOptions,
  type LabStatus,
  type Provider,
  type ServerHost,
} from "@/lib/tauri";
import type { LocalVmForm } from "@/features/labs/run-target";

// Where a lab can run, from its runtime (catalogue) and what this machine has ready.

const SERVERS = new Set<string>(SERVER_PROVIDERS);

/** Whether `p` is a hypervisor a lab's VMs run on here (the provider a local VM lab's status
 *  names): one of the backend's local providers. */
export const isLocalHypervisor = (p: string | null | undefined): p is Provider => !!p && (LOCAL_PROVIDERS as readonly string[]).includes(p);

/** Whether the lab is built for this CPU (no architectures listed = any). */
export const runsNatively = (rt: LabRuntimeInfo, hostArch: string) => !rt.architectures.length || rt.architectures.includes(hostArch);

/** Whether `rt` runs natively here, on `hostArch`: also when the host isn't known yet (the report
 *  not in) or the lab's runtime isn't, never a wrong "emulated". */
export const runsNativelyHere = (rt: LabRuntimeInfo | null | undefined, hostArch: string) => !hostArch || !rt || runsNatively(rt, hostArch);

/** The ways a lab can run: the backend's capability flags when present, else derived from the
 *  single runtime kind so this is correct before a backend that records them. A dual-form lab has
 *  more than one true (e.g. docker and vm), and the launcher offers a place for each. */
const ways = (rt: LabRuntimeInfo) => ({
  docker: rt.docker ?? rt.runtime === "DOCKER",
  vm: rt.vm ?? rt.runtime === "VM",
  cloud: rt.cloud ?? rt.runtime === "CLOUD",
});

/** The hypervisors a form of the lab runs on here, as the backend's run options say (`paths`:
 *  the helper VM's, or one VM per machine's, which for VMs built for another CPU are the emulators
 *  alone). */
export const formProviders = (paths: LabRunOptions["vms"]): Provider[] => paths.flatMap((p) => (p.provider ? [p.provider] : []));

/** Whether an emulator (QEMU) is among the hypervisors `ready` here (the machine report's). */
export const emulatorReady = (ready: readonly Provider[]) => ready.some((p) => EMULATORS.includes(p));

/** The hypervisors on this machine that can run a VM lab's VMs: as the backend's run options say
 *  (`options`, once read: VMs built for another CPU on an emulator only, by the backend's rule),
 *  else, until then, those the catalogue lists, a VM lab built for another CPU only emulated. A
 *  container lab's helper VM is the run options' too (`formProviders`), and none for cloud
 *  services. */
export function localProviders(rt: LabRuntimeInfo, hostArch?: string, options?: LabRunOptions | null): Provider[] {
  if (rt.runtime !== "VM") return options ? formProviders(options.dockerVm) : [];
  if (options) return formProviders(options.vms);
  // A VM edition built for another CPU runs here only emulated (whatever hypervisors it lists).
  return hostArch && !runsNatively(rt, hostArch) ? [...EMULATORS] : rt.providers.filter(isLocalHypervisor);
}

/** The cloud paths the backend's run options give the lab, every form. */
const cloudPaths = (o: LabRunOptions) => [...o.cloudDocker, ...o.cloudVm, ...o.cloudServices];

/** Whether a saved server or cloud account can run the lab: as the backend's run options say
 *  (`options`: a path on its kind of server or its cloud), else, until they are read, when the
 *  catalogue lists its provider. */
export function hostSupports(rt: LabRuntimeInfo | null, h: ServerHost, options?: LabRunOptions | null): boolean {
  if (!options) return !!rt?.providers.includes(h.provider);
  const paths = isCloudProvider(h.provider) ? cloudPaths(options) : options.servers;
  return paths.some((p) => p.provider === h.provider);
}

/** A place a lab runs: its words are `labs.places.<key>`, its icon `PLACE_ICONS` (run-place-icons). */
export type PlaceKey = NonNullable<LabStatus["place"]>;

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
  hosts.some((h) => isCloudProvider(h.provider) && hostSupports(rt, h, options));

/** Every place a lab could run, and its state (see [`PlaceState`]). Its words are
 *  `labs.places.<key>`. Whether the lab runs there at all is the backend's run options' word once
 *  they are read (`setup.options`), the catalogue's until then. `setup` is also what the launcher
 *  has ready, used to tell a place that can run now from one the lab supports but the launcher
 *  isn't configured for.
 *  A VM lab built for another CPU (x86 Windows on Apple Silicon) runs on this machine only
 *  emulated, so "VM on this machine" needs `emulates` (QEMU ready); containers run emulated
 *  anyway. */
export function runPlaces(rt: LabRuntimeInfo, hostArch?: string, emulates = false, setup: LauncherSetup = {}): { key: PlaceKey; state: PlaceState }[] {
  const { readyVms = [], hosts = [], options = null } = setup;
  const w = ways(rt);
  // Cloud services (no container or VM edition) run in the player's cloud account only.
  if (w.cloud && !w.docker && !w.vm) return cloudPlaces(rt, hosts, options);
  // A container lab runs in a helper VM here where the backend's run options say, else (until
  // they are read) wherever the catalogue lists no server or cloud only.
  const local = options ? options.dockerVm.length > 0 : rt.providers.some((p) => !SERVERS.has(p) && !isCloudProvider(p));
  // The VM edition built for another CPU runs here only emulated (containers run emulated anyway):
  // the backend's word once its run options are read, the catalogue's until then.
  const vmForeignCpu = w.vm && (options ? options.emulated : !!hostArch && !runsNatively(rt, hostArch));
  const localVmSupported = (w.vm || (w.docker && local)) && (!vmForeignCpu || emulates);
  const localVmReady = readyVms.some((p) => localProviders(rt, hostArch, options).includes(p));
  const containerSupported = options ? options.docker !== null : w.docker;
  const serverSupported = options ? options.servers.length > 0 : (w.vm || w.docker) && rt.providers.some((p) => SERVERS.has(p));
  const serverReady = hosts.some((h) => SERVERS.has(h.provider) && hostSupports(rt, h, options));
  const cloudSupported = options ? cloudPaths(options).length > 0 : rt.providers.some((p) => isCloudProvider(p));
  return [
    { key: "container", state: placeState(containerSupported, true) },
    { key: "local_vm", state: placeState(localVmSupported, localVmReady) },
    { key: "server", state: placeState(serverSupported, serverReady) },
    { key: "cloud", state: placeState(cloudSupported, cloudConfigured(rt, hosts, options)) },
  ];
}

/** Cloud services: the player's cloud account, nowhere else (configurable until one is connected;
 *  unsupported when the backend's run options give it no cloud, Isoloom refusing it there). */
function cloudPlaces(rt: LabRuntimeInfo, hosts: ServerHost[], options: LabRunOptions | null): { key: PlaceKey; state: PlaceState }[] {
  return [
    { key: "container", state: "unsupported" },
    { key: "local_vm", state: "unsupported" },
    { key: "server", state: "unsupported" },
    { key: "cloud", state: placeState(options ? options.cloudServices.length > 0 : true, cloudConfigured(rt, hosts, options)) },
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
export const cloudNames = (providers: readonly string[]): string[] => providers.filter((p): p is Provider => isCloudProvider(p)).map((p) => PROVIDER_NAMES[p]);

/** The player's accounts a cloud lab can run on: of a cloud it lists. */
export const cloudAccountsFor = (rt: LabRuntimeInfo | null | undefined, hosts: ServerHost[], options?: LabRunOptions | null) =>
  rt && isCloudLab(rt) ? hosts.filter((h) => isCloudProvider(h.provider) && hostSupports(rt, h, options)) : [];

/** A container lab's VM form here (`local-vm`: one helper VM, `local-vms`: one VM per machine) on
 *  hypervisor `provider`. */
export const vmFormTarget = (form: LocalVmForm["kind"], provider: Provider): LocalVmForm => ({ kind: form, provider });
