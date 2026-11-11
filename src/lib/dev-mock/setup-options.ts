// The guided setup's options as the core works them out from a machine check (Rust's
// `machine::setup::options`), for the preview's report and the tests' reports: the preview changes
// its report as its installs run, with no core to ask. The setup window itself never works these
// out; `setup-options.test.ts` holds this stand-in to the core's answer on every machine shape the
// core writes (`SETUP_SHAPES`).
import type { Dependency, DockerEngine, Provider, ProviderStatus, SetupAction, SetupOptions, SystemReport } from "@/lib/tauri";
import { DOCKER_ENGINE_URLS } from "@/lib/tauri/generated/DockerEngines";

/** The engines offered per OS, in the setup's order (Rust's `engines_on`). */
const ENGINES_ON: Partial<Record<string, DockerEngine[]>> = {
  macos: ["docker-desktop", "orbstack", "colima"],
  windows: ["docker-desktop"],
};
const enginesOn = (os: string): DockerEngine[] => ENGINES_ON[os] ?? ["docker-desktop", "docker-engine", "colima"];

const recommended = (os: string): DockerEngine => (os === "macos" || os === "windows" ? "docker-desktop" : "docker-engine");

const dockerReady = (r: Machine) => r.docker.installed && r.dockerRunning;

/** Where the player installs a tool themselves (Rust's `own_install_url`). */
const OWN_INSTALL: Partial<Record<Dependency, string>> = {
  docker: "https://docs.docker.com/engine/install/",
  vagrant: "https://developer.hashicorp.com/vagrant/install",
  virtualbox: "https://www.virtualbox.org/wiki/Linux_Downloads",
  qemu: "https://www.qemu.org/download/",
  libvirt: "https://libvirt.org/",
  "vmware-utility": "https://developer.hashicorp.com/vagrant/install/vmware",
};

/** Where a hypervisor the setup doesn't install comes from (Rust's `vendor_url`). */
const VENDOR: Partial<Record<Provider, string>> = {
  vmware_desktop: "https://www.vmware.com/products/desktop-hypervisor/workstation-and-fusion",
  parallels: "https://www.parallels.com/products/desktop/",
  hyperv: "https://learn.microsoft.com/virtualization/hyper-v-on-windows/quick-start/enable-hyper-v",
};

/** A machine as the check reports it, its setup options aside. */
type Machine = Omit<SystemReport, "setup">;

function install(r: Machine, dependency: Dependency): SetupAction {
  if (r.os === "linux" && !r.pkgManager.installed) return { kind: "manual", reason: "distribution", dependency, url: OWN_INSTALL[dependency] ?? null };
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

function installable(provider: Provider, os: string): Dependency | null {
  if (provider === "virtualbox" || provider === "qemu") return provider;
  if (provider === "utm" && os === "macos") return "utm";
  if (provider === "libvirt" && os === "linux") return "libvirt";
  if (provider === "hyperv" && os === "windows") return "hyperv";
  return null;
}

const cantRun = (p: ProviderStatus) => p.hypervisor === true && !p.available && !!p.reason && p.needs.length === 0;

/** The groups libvirt's socket and /dev/kvm are opened through (Rust's `LIBVIRT_GROUPS`). */
const LIBVIRT_GROUPS = ["libvirt", "kvm"];

/** The groups a player joins to use `provider` on `os` (Rust's `login_groups`): libvirt's, and
 *  /dev/kvm's for QEMU on Linux (`kvm`, its usual name; the core reads the device's own). */
function loginGroups(provider: Provider, os: string): string[] {
  if (provider === "libvirt") return LIBVIRT_GROUPS;
  if (provider === "qemu" && os === "linux") return ["kvm"];
  return [];
}

function get(r: Machine, p: ProviderStatus, hypervRestart: boolean): SetupAction[] {
  const groups = loginGroups(p.provider, r.os);
  if (p.hypervisor === true && r.groupsPending.some((g) => groups.includes(g))) return [{ kind: "relogin" }];
  if (p.hypervisor === true) return cantRun(p) && p.reason ? [{ kind: "blocked", reason: p.reason }] : [];
  if (p.provider === "hyperv" && hypervRestart) return [{ kind: "restart", feature: "hyperv" }];
  const dependency = installable(p.provider, r.os);
  return [dependency ? install(r, dependency) : { kind: "manual", reason: "vendor", dependency: null, url: VENDOR[p.provider] ?? null }];
}

function vagrant(r: Machine, p: ProviderStatus): SetupAction[] {
  return p.needs.flatMap((need): SetupAction[] => {
    if (need === "vagrant") return [install(r, "vagrant")];
    if (need === "plugin") return p.plugin ? [{ kind: "plugin", plugin: p.plugin }] : [];
    return [install(r, "vmware-utility")];
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
