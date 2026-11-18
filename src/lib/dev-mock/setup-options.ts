// The guided setup's options as the core works them out from a machine check (Rust's
// `machine::setup::options`), for the preview's report and the tests' reports: the preview changes
// its report as its installs run, with no core to ask. The setup window itself never works these
// out; `setup-options.test.ts` holds this stand-in to the core's answer on every machine shape the
// core writes (`SETUP_SHAPES`).
import type { Dependency, DockerEngine, Os, Provider, ProviderStatus, SetupAction, SetupOptions, SystemReport } from "@/lib/tauri";
import { DOCKER_ENGINE_URLS } from "@/lib/tauri/generated/DockerEngines";
import { ENGINES_ON, INSTALLS_AS, OWN_INSTALL_URLS, RECOMMENDED_ENGINE, VENDOR_URLS } from "@/lib/tauri/generated/SetupTables";

const enginesOn = (os: Os): readonly DockerEngine[] => ENGINES_ON[os];

const recommended = (os: Os): DockerEngine => RECOMMENDED_ENGINE[os];

const dockerReady = (r: Machine) => r.docker.installed && r.dockerRunning;

/** A machine as the check reports it, its setup options aside. */
type Machine = Omit<SystemReport, "setup">;

function install(r: Machine, dependency: Dependency): SetupAction {
  if (r.os === "linux" && !r.pkgManager.installed) return { kind: "manual", reason: "distribution", dependency, url: OWN_INSTALL_URLS[dependency] ?? null };
  return { kind: "install", dependency };
}

function wsl(r: Machine): SetupAction[] {
  const state = r.wsl?.state ?? "unknown";
  if (r.os !== "windows" || state === "ready" || dockerReady(r)) return [];
  if (state === "needsReboot") return [{ kind: "restart", feature: "wsl" }];
  return [install(r, "wsl")];
}

function engineActions(r: Machine, engine: DockerEngine): SetupAction[] {
  if (dockerReady(r) && r.dockerEngine === engine) return [];
  if (r.dockerEnginesRunning.includes(engine)) return [{ kind: "useEngine", engine }];
  if (r.dockerDenied && engine === "docker-engine") return [{ kind: "relogin" }];
  if (r.docker.installed && !dockerReady(r)) return [{ kind: "startEngine", engine }];
  if (engine === recommended(r.os) && !r.docker.installed) return [install(r, "docker")];
  return [{ kind: "manual", reason: "vendor", dependency: null, url: DOCKER_ENGINE_URLS[engine] }];
}

const installable = (provider: Provider, os: Os): Dependency | null => INSTALLS_AS[os][provider] ?? null;

const cantRun = (p: ProviderStatus) => p.hypervisor === true && !p.available && !!p.reason && p.needs.length === 0;

/** The groups libvirt's socket and /dev/kvm are opened through (Rust's `LIBVIRT_GROUPS`). */
const LIBVIRT_GROUPS = ["libvirt", "kvm"];

/** The groups a player joins to use `provider` on `os` (Rust's `login_groups`): libvirt's, and
 *  /dev/kvm's for QEMU on Linux (`kvm`, its usual name; the core reads the device's own). */
function loginGroups(provider: Provider, os: Os): string[] {
  if (provider === "libvirt") return LIBVIRT_GROUPS;
  if (provider === "qemu" && os === "linux") return ["kvm"];
  return [];
}

function get(r: Machine, p: ProviderStatus, hypervRestart: boolean): SetupAction[] {
  // KVM holding the CPU's virtualization: VirtualBox's install when it is missing, then KVM set to
  // share it (or the restart that applies that).
  const reason = p.reason;
  if (reason?.code === "kvmHoldsVirtualization" || reason?.code === "kvmSharesAfterRestart") {
    const first: SetupAction[] = p.hypervisor === true ? [] : [getHypervisor(r, p)];
    return [...first, reason.code === "kvmSharesAfterRestart" ? { kind: "blocked", reason } : { kind: "shareVirtualization" }];
  }
  const groups = loginGroups(p.provider, r.os);
  if (p.hypervisor === true && r.groupsPending.some((g) => groups.includes(g))) return [{ kind: "relogin" }];
  if (p.hypervisor === true) return cantRun(p) && p.reason ? [{ kind: "blocked", reason: p.reason }] : [];
  if (p.provider === "hyperv" && hypervRestart) return [{ kind: "restart", feature: "hyperv" }];
  return [getHypervisor(r, p)];
}

/** What brings hypervisor `p` here: its install, else its vendor's site. */
function getHypervisor(r: Machine, p: ProviderStatus): SetupAction {
  const dependency = installable(p.provider, r.os);
  return dependency ? install(r, dependency) : { kind: "manual", reason: "vendor", dependency: null, url: VENDOR_URLS[p.provider] ?? null };
}

/** What Vagrant needs to drive `p`, as the core plans it: a provider driven through VMware's
 *  utility gets neither the utility nor its plugin before its hypervisor is installed, and its
 *  plugin only with the utility's install in the same plan (or once the utility is there): the
 *  plugin without the utility breaks Vagrant for every provider. */
function vagrant(r: Machine, p: ProviderStatus): SetupAction[] {
  const throughUtility = p.provider === "vmware_desktop";
  if (throughUtility && p.hypervisor !== true) return p.needs.includes("vagrant") ? [install(r, "vagrant")] : [];
  const utility = install(r, "vmware-utility");
  const utilityInstalls = !p.needs.includes("vmwareUtility") || utility.kind === "install";
  return p.needs.flatMap((need): SetupAction[] => {
    if (need === "vagrant") return [install(r, "vagrant")];
    if (need === "vmwareUtility") return [utility];
    if (throughUtility && !utilityInstalls) return [];
    return p.plugin ? [{ kind: "plugin", plugin: p.plugin }] : [];
  });
}

/** What each choice of the setup takes on the machine `r` reports (`hypervRestart`: Hyper-V was
 *  turned on in this boot). */
export function setupOptions(r: Machine, hypervRestart = false): SetupOptions {
  const offered = enginesOn(r.os);
  const engine = r.dockerEngine === null ? recommended(r.os) : offered.includes(r.dockerEngine) ? r.dockerEngine : offered[0];
  const local = r.vmProviders.filter((p) => !p.remote);
  const hypervisors = local.map((p) => ({ provider: p.provider, get: get(r, p, hypervRestart), vagrant: vagrant(r, p) }));
  const installed = (provider: Provider) => local.some((p) => p.provider === provider && p.hypervisor === true);
  const hypervisor =
    hypervisors.find((o) => installed(o.provider) && o.get.length === 0) ??
    hypervisors.find((o) => installed(o.provider)) ??
    hypervisors.find((o) => o.get[0]?.kind === "install" && !o.get.some((a) => a.kind === "shareVirtualization")) ??
    hypervisors.find((o) => o.get[0]?.kind === "install") ??
    hypervisors.at(0);
  return {
    wsl: wsl(r),
    recommendedEngine: recommended(r.os),
    engine,
    engines: offered.map((e) => ({ engine: e, actions: engineActions(r, e) })),
    hypervisor: hypervisor?.provider ?? null,
    hypervisors,
  };
}

/** `r` with its setup options worked out again, as the core's next check would send them. */
export const withSetup = (r: Machine, hypervRestart = false): SystemReport => ({ ...r, setup: setupOptions(r, hypervRestart) });
