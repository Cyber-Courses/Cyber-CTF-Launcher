/**
 * Browser preview of the UI (development only): `pnpm dev`, then open
 * http://localhost:3000/?mock. Outside Tauri there is no Rust core, so this answers the
 * commands the screens call with plausible sample data (a signed-in player, two running labs,
 * a server, an AWS account); Settings and the server setup open in new tabs. Never loaded in
 * a production build or inside the app. The labs and their objectives are in dev-mock-labs.ts.
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { CommandError } from "@/lib/errors";
import { GB } from "@/lib/format";
import type { LabStatus, Provider, ProviderStatus, ServerHost, ServerList, SystemReport } from "@/lib/tauri";
import type { Args, Handler, Handlers } from "@/lib/dev-mock-types";
import { apiQuery, invoiceStatus, labCommands, labs, stopped } from "@/lib/dev-mock-labs";
import { clearOnboarded, markOnboarded } from "@/components/shell/onboarded-flag";

const report: SystemReport = {
  os: "macos",
  arch: "arm64",
  pkgManager: { name: "brew", installed: true, version: "4.4.0" },
  docker: { installed: true, version: "27.3.1" },
  dockerRunning: true,
  dockerDenied: false,
  dockerDeniedHint: null,
  dockerEngine: "docker-desktop",
  dockerEnginesRunning: ["docker-desktop"],
  dockerCompose: { installed: true, version: "2.29.7" },
  vagrant: { installed: true, version: "2.4.3" },
  terraform: { installed: true, version: "1.9.8" },
  ovftool: { installed: false, version: null },
  cloudClis: { aws: { installed: true, version: "2.18.0" }, azure: { installed: false, version: null }, gcloud: { installed: false, version: null } },
  vmProviders: [
    { provider: "virtualbox", remote: false, available: true, hypervisor: true, plugin: null, pluginInstalled: true, reason: null },
    {
      provider: "vmware_desktop",
      remote: false,
      available: false,
      hypervisor: false,
      plugin: "vagrant-vmware-desktop",
      pluginInstalled: false,
      reason: "VMware Fusion is not installed",
    },
    { provider: "utm", remote: false, available: false, hypervisor: false, plugin: "vagrant_utm", pluginInstalled: false, reason: "UTM is not installed" },
    { provider: "vmware_esxi", remote: true, available: true, hypervisor: null, plugin: "vagrant-vmware-esxi", pluginInstalled: true, reason: null },
    { provider: "proxmox", remote: true, available: true, hypervisor: null, plugin: null, pluginInstalled: true, reason: null },
  ],
  targets: [
    { target: "docker", cloud: null, ready: true, summary: "Docker Desktop 27.3.1", notes: ["docker 27.3.1"] },
    { target: "vagrant", cloud: null, ready: true, summary: "Vagrant 2.4.3 with VirtualBox", notes: ["vagrant 2.4.3"] },
  ],
  wsl: null,
};

/** A saved server or cloud account: the fields only some providers use left empty. */
const savedHost = (h: Pick<ServerHost, "id" | "name" | "provider" | "host" | "port" | "username"> & Partial<ServerHost>): ServerHost => ({
  datastore: null,
  network: null,
  node: null,
  insecureTls: false,
  autoStopHours: null,
  useCliCreds: false,
  awsProfile: null,
  monthlyLimit: null,
  gcpProject: null,
  ...h,
});

/** The saved servers, a new copy per call (as the backend answers). */
const servers = (): ServerList => ({
  default: "pve",
  hosts: [
    savedHost({
      id: "pve",
      name: "homelab-pve",
      provider: "proxmox",
      host: "192.168.1.20",
      port: 8006,
      username: "root@pam",
      datastore: "local-lvm",
      network: "vmbr0",
      node: "pve",
      insecureTls: true,
      autoStopHours: 4,
    }),
    savedHost({
      id: "esxi",
      name: "office-esxi",
      provider: "vmware_esxi",
      host: "10.0.4.12",
      port: 443,
      username: "root",
      datastore: "datastore1",
      network: "VM Network",
      node: null,
      insecureTls: true,
      autoStopHours: null,
    }),
    savedHost({
      id: "aws1",
      name: "AWS sandbox",
      provider: "aws",
      host: "eu-west-3",
      port: 0,
      username: "",
      datastore: null,
      network: null,
      node: null,
      insecureTls: false,
      autoStopHours: 4,
      useCliCreds: true,
      awsProfile: "default",
    }),
  ],
});

let cpu = 38;
let sharedFolder = { enabled: false, path: "/Users/florian/cyberctf-share", mountPoint: "/workspace/share" };
let isoloomRegistry = true;

/** The app opens Settings and the server setup in their own windows; here, a browser tab. The
 *  preview's switches (see `Scenario`) go along, so the new tab shows the same machine. */
function openWindow(path: string) {
  const carried = new URLSearchParams(window.location.search);
  for (const own of ["mock", "window", "id", "kind", "runtime", "step", "onboarding", "link"]) carried.delete(own);
  const extra = carried.toString();
  window.open(`${path}${path.includes("?") ? "&" : "?"}mock${extra ? `&${extra}` : ""}`, "_blank");
}

/** The player, this install and the API. */
const accountCommands = {
  auth_status: () => ({ loggedIn: true, name: "Florian Amette", email: "florian@cyberctf.org" }),
  auth_login: () => ({ loggedIn: true, name: "Florian Amette", email: "florian@cyberctf.org" }),
  agent_info: () => ({ installId: "c0ffee", name: "florian-mbp", arch: "arm64", capabilities: ["docker", "vagrant"] }),
  api_query: apiQuery,
} satisfies Handlers;

/** The attack box (a container or the attack VM), up on the lab network. */
const attackBox = () => ({ imagePresent: true, running: true, ip: "10.42.0.9", labNetwork: "default", shellCmd: "docker exec -it attack-box zsh" });

/** This machine: its report, live usage, what runs and is stored on it, the attack box. */
const machineCommands = {
  system_check: () => report,
  machine_metrics: () => {
    cpu = Math.max(8, Math.min(92, cpu + (Math.random() - 0.5) * 14));
    return { cpu, memUsed: 19.5 * GB, memTotal: 32 * GB, diskUsed: 370 * GB, diskTotal: 500 * GB, uptimeSecs: 86400 * 3, cores: 8, containers: 6 };
  },
  machine_workloads: () => [
    { id: "lab-invoice", kind: "docker", count: 3, memBytes: 0.6 * GB, provider: null },
    { id: "lab-goad", kind: "vm", count: 3, memBytes: 12 * GB, provider: "virtualbox" },
  ],
  machine_storage: () => ({
    images: [
      { name: "cyberctf/attack-box:latest", bytes: 4.1 * GB },
      { name: "postgres:16", bytes: 0.43 * GB },
    ],
    boxes: [{ name: "StefanScherer/windows_2019", bytes: 11.2 * GB }],
    plugins: [{ name: "Terraform provider plugins", bytes: 0.6 * GB }],
  }),
  exegol_status: () => attackBox(),
  attack_vm_status: () => attackBox(),
  image_download_size: () => 4.1 * GB,
  installed_tools: () => [],
  shared_folder_get: () => sharedFolder,
  shared_folder_set: (args) => {
    sharedFolder = { ...sharedFolder, enabled: !!args?.enabled, path: String(args?.path ?? sharedFolder.path) };
    return sharedFolder;
  },
  isoloom_registry_get: () => isoloomRegistry,
  isoloom_registry_set: (args) => {
    isoloomRegistry = !!args?.enabled;
    return isoloomRegistry;
  },
} satisfies Handlers;

/** The saved servers and the AWS account. */
const serverCommands = {
  server_list: servers,
  server_capacity: () => ({ cores: 32, memTotal: 64 * GB, memFree: 42 * GB }),
  server_test: () => ({ ok: true, reachable: true, authenticated: true, latencyMs: 12, message: "Connected", checks: [] }),
  aws_month_to_date_cost: () => 6.4,
  aws_cli_identity: () => "arn:aws:iam::482100000193:user/florian",
  aws_profiles: () => ["default"],
} satisfies Handlers;

/** The app's windows and Tauri plugins. */
const appCommands = {
  open_settings: () => {
    openWindow("/?window=settings");
    return null;
  },
  server_open_setup: (args) => {
    const id = args?.id ? `?id=${encodeURIComponent(String(args.id))}` : args?.kind === "cloud" ? "?kind=cloud" : "";
    openWindow(`/server-setup${id}`);
    return null;
  },
  "plugin:app|version": () => "0.3.2",
  "plugin:notification|is_permission_granted": () => false,
  "plugin:event|listen": () => 1,
} satisfies Handlers;

// A Map, not an object lookup: a command named like an Object.prototype member is unknown too.
const HANDLERS = new Map<string, Handler>(Object.entries({ ...accountCommands, ...labCommands, ...machineCommands, ...serverCommands, ...appCommands }));

/** The mock's answer to a command; null for one it doesn't know. */
export function answer(cmd: string, args: Args): unknown {
  const handler = HANDLERS.get(cmd);
  return handler ? handler(args) : null;
}

/* ------------------------------------------------------------------ browser scenarios */

/**
 * Switches for the browser preview, read from the URL next to `?mock` (the end-to-end tests use
 * them; `answer` above stays the plain sample data the component tests build on):
 *  - `os=macos|windows|linux`, `arch=arm64|x86_64`: the machine the system check reports.
 *  - `machine=fresh`: nothing installed yet (no package manager, engine, Vagrant or hypervisor).
 *  - `onboarding`: the first-run onboarding shows (on every load with the switch).
 *  - `auth=out`: signed out until "Sign in" is used.
 *  - `check=down`: every system check fails until the test brings the machine back
 *    (`window.__CYBERCTF_MOCK_MACHINE_UP__()`), so no background re-check can answer before the
 *    test has seen the failure.
 *  - `launch=fail`: the first lab start of each page load fails, after a few lines of output.
 *  - `pm=later`: the package manager is missing from the first round of checks of each page load,
 *    then there (as if the player ran its install command while the setup window was open).
 *  - `install=fail`: the first one-click install of each page load fails. Otherwise installs
 *    stream an installer's output and succeed: the next system check reports the tool installed.
 *  - `deploying=none`: no lab is deploying at load (GOAD Light otherwise is).
 *  - `cloud=none`: no cloud account saved (until one is saved in the setup).
 *  - `link=<url>`: the app was opened with this link (`cyberctf://labs/<slug>`).
 * Labs started or stopped here stay so until a reload: Docker labs stream a Compose log, VM labs
 * a Vagrant log (on this machine's hypervisor or a saved server; pause, shut down, resume and
 * re-run provisioning work), cloud labs a Terraform log (outputs and an auto-stop), hosted labs
 * go from requested to running on the next poll. The attack box shell is a stand-in that answers
 * `id`, `whoami`, `hostname`, `ip -br a`, and ends on `exit`. Events emitted in one tab (Settings asking
 * the main window to show the onboarding again, a server saved in the setup) reach the others.
 */
export type Scenario = {
  os: "macos" | "windows" | "linux";
  arch: "arm64" | "x86_64";
  fresh: boolean;
  onboarding: boolean;
  signedOut: boolean;
  checkDown: boolean;
  launchFails: boolean;
  pmLater: boolean;
  installFails: boolean;
  noDeploying: boolean;
  noCloud: boolean;
  link: string | null;
};

export function scenarioFrom(search: string): Scenario {
  const q = new URLSearchParams(search);
  const os = q.get("os");
  return {
    os: os === "windows" || os === "linux" ? os : "macos",
    arch: q.get("arch") === "x86_64" ? "x86_64" : "arm64",
    fresh: q.get("machine") === "fresh",
    onboarding: q.has("onboarding"),
    signedOut: q.get("auth") === "out",
    checkDown: q.get("check") === "down",
    launchFails: q.get("launch") === "fail",
    pmLater: q.get("pm") === "later",
    installFails: q.get("install") === "fail",
    noDeploying: q.get("deploying") === "none",
    noCloud: q.get("cloud") === "none",
    link: q.get("link"),
  };
}

const notInstalled = { installed: false, version: null };

/** The local hypervisors each OS reports (the remote ones are the same everywhere). */
function localProviders(os: Scenario["os"], fresh: boolean) {
  const p = (provider: Provider, plugin: string | null = null): ProviderStatus => ({
    provider,
    remote: false,
    available: !fresh && provider === "virtualbox",
    hypervisor: fresh ? false : provider === "virtualbox",
    plugin,
    pluginInstalled: !plugin,
    reason: !fresh && provider === "virtualbox" ? null : "Not installed",
  });
  if (os === "windows") return [p("virtualbox"), p("vmware_desktop", "vagrant-vmware-desktop"), { ...p("hyperv"), hypervisor: null }];
  if (os === "linux") return [p("virtualbox"), p("libvirt", "vagrant-libvirt"), p("qemu", "vagrant-qemu")];
  return [p("virtualbox"), p("vmware_desktop", "vagrant-vmware-desktop"), p("utm", "vagrant_utm"), p("parallels", "vagrant-parallels")];
}

/** The system check's answer for a scenario. On an Apple Silicon Mac VirtualBox can't run, so a
 *  ready machine there has UTM instead. */
export function scenarioReport(sc: Scenario): SystemReport {
  const remote = report.vmProviders.filter((p) => p.remote);
  let local = localProviders(sc.os, sc.fresh);
  if (!sc.fresh && sc.os === "macos" && sc.arch === "arm64") {
    local = local.map((p) =>
      p.provider === "virtualbox"
        ? { ...p, available: false, hypervisor: false, reason: "VirtualBox does not run on Apple Silicon" }
        : p.provider === "utm"
          ? { ...p, available: true, hypervisor: true, pluginInstalled: true, reason: null }
          : p,
    );
  }
  const pm = sc.os === "windows" ? "winget" : sc.os === "linux" ? "apt" : "brew";
  const engine = sc.os === "linux" ? "docker-engine" : "docker-desktop";
  return {
    ...report,
    os: sc.os,
    arch: sc.arch,
    pkgManager: { name: pm, installed: !sc.fresh, version: sc.fresh ? null : "1.9.0" },
    docker: sc.fresh ? notInstalled : report.docker,
    dockerRunning: !sc.fresh,
    dockerDenied: false,
    dockerDeniedHint: null,
    dockerEngine: sc.fresh ? null : engine,
    dockerEnginesRunning: sc.fresh ? [] : [engine],
    dockerCompose: sc.fresh ? notInstalled : report.dockerCompose,
    vagrant: sc.fresh ? notInstalled : report.vagrant,
    terraform: sc.fresh ? notInstalled : report.terraform,
    cloudClis: sc.fresh ? { aws: notInstalled, azure: notInstalled, gcloud: notInstalled } : report.cloudClis,
    vmProviders: [...local, ...remote.map((p) => (sc.fresh ? { ...p, available: false, pluginInstalled: false } : p))],
    targets: sc.fresh ? [] : report.targets,
  };
}

/** The system check once the preview's one-click installs ran (`installed`: dependencies and
 *  Vagrant plugins), with the package manager there or not as `pm` says (null: as reported). */
export function withInstalls(r: SystemReport, installed: ReadonlySet<string>, pm: boolean | null = null): SystemReport {
  const pkgManager = pm === null ? r.pkgManager : { ...r.pkgManager, installed: pm, version: pm ? "4.4.0" : null };
  if (!installed.size) return { ...r, pkgManager };
  const has = (tool: string) => installed.has(tool);
  const engine = r.os === "linux" ? "docker-engine" : "docker-desktop";
  const docker = has("docker");
  const vagrant = has("vagrant") ? { installed: true, version: "2.4.3" } : r.vagrant;
  const vmProviders = r.vmProviders.map((p) => {
    if (p.remote) return p;
    const hypervisor = has(p.provider) || p.hypervisor;
    const pluginInstalled = p.pluginInstalled || (!!p.plugin && has(p.plugin));
    const available = hypervisor === true && vagrant.installed && pluginInstalled;
    return { ...p, hypervisor, pluginInstalled, available, reason: available ? null : p.reason };
  });
  return {
    ...r,
    pkgManager,
    docker: docker ? { installed: true, version: "27.3.1" } : r.docker,
    dockerRunning: docker || r.dockerRunning,
    dockerEngine: docker ? engine : r.dockerEngine,
    dockerEnginesRunning: docker ? [engine] : r.dockerEnginesRunning,
    dockerCompose: docker ? { installed: true, version: "2.29.7" } : r.dockerCompose,
    vagrant,
    terraform: has("terraform") ? { installed: true, version: "1.9.8" } : r.terraform,
    vmProviders,
  };
}

/** What each one-click install prints, as the core streams it. */
function installLines(what: string): string[] {
  if (what.startsWith("vagrant-") || what.startsWith("vagrant_"))
    return [`Installing the '${what}' plugin. This can take a few minutes...`, `Installed the plugin '${what} (0.3.1)'!`];
  return [`==> Downloading ${what}`, `==> Installing ${what}`, `🍺  ${what} was successfully installed!`];
}

/** Where a VM or cloud lab started in the preview runs, and whether it is parked. */
type Placement = { provider: string; host: Host | null; parked: "pause" | "shutdown" | null; startedAt: number };

/** A VM lab's machines (GOAD Light's, for any VM lab). */
const VM_MACHINES = [
  { name: "dc01", ip: "192.168.56.10", image: "StefanScherer/windows_2019" },
  { name: "srv02", ip: "192.168.56.22", image: "StefanScherer/windows_2019" },
];

/** A VM lab's status: its machines on the lab network, running, saved or powered off. */
function vmStatus(p: Placement): LabStatus {
  const state = p.parked === "pause" ? "saved" : p.parked === "shutdown" ? "poweroff" : "running";
  return {
    ...stopped,
    running: !p.parked,
    parked: p.parked,
    host: p.host ? String(p.host.name) : null,
    place: p.host ? "server" : "local_vm",
    provider: p.provider,
    networks: [{ name: "lab", subnet: "192.168.56.0/24", internal: true }],
    machines: VM_MACHINES.map((m) => ({
      name: m.name,
      state,
      image: m.image,
      ip: m.ip,
      ports: [],
      interfaces: [{ network: "lab", ip: m.ip }],
      services: [{ name: "ldap", kind: "directory", ports: [389] }],
      infra: false,
    })),
  };
}

/** A cloud lab's status: up in the account, with its outputs and when it stops itself. */
function cloudStatus(p: Placement): LabStatus {
  const hours = Number(p.host?.autoStopHours ?? 0);
  return {
    ...stopped,
    running: true,
    host: p.host ? String(p.host.name) : null,
    place: "cloud",
    provider: p.provider,
    expiresAt: hours ? Math.floor(p.startedAt / 1000) + hours * 3600 : null,
    outputs: [
      { name: "access_key_id", value: "AKIAZ7EXAMPLE4SSRF01", sensitive: false },
      { name: "secret_key", value: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYSSRF", sensitive: true },
    ],
    message: "Sign in with access_key_id and secret_key, then find the Lambda function.",
  };
}

const vagrantMachines = (machine?: string | null) => VM_MACHINES.filter((m) => !machine || m.name === machine).map((m) => m.name);

/** A hosted session (Cyber CTF runs the lab), as CyberBackend reports it. */
type Session = { id: string; labId: string; target: "hosted"; state: string; endpoints: { port: number; url: string }[]; message: null; expiresAt: string };

/** What `browserAnswer` changes as the preview is used: labs started and stopped, servers saved,
 *  the sign-in, the failures still to come. */
type Live = {
  running: Set<string>;
  signedIn: boolean;
  /** System checks fail (`check=down`) until the test brings the machine back. */
  checkDown: boolean;
  launchFails: boolean;
  /** The operations in flight on each lab, as the core reports them. */
  ops: Map<string, "launch" | "stop" | "pause" | "shutdown" | "resume" | "provision">;
  /** Tools the one-click installs added (dependencies, Vagrant plugins). */
  installed: Set<string>;
  installFails: boolean;
  /** When the first system check of the page load was asked (ms), for `pm=later`. */
  firstCheckAt: number;
  /** VM and cloud labs started here: where they run. */
  placed: Map<string, Placement>;
  /** The hosted session, once one is requested. */
  hosted: Session | null;
};

type Stream = { onmessage?: (m: unknown) => void };
const send = (args: Record<string, unknown> | undefined, m: unknown) => (args?.logs as Stream | undefined)?.onmessage?.(m);
/** A failed command: the Rust core rejects with the error's code and message, not an Error. */
const rejected = (message: string, code: CommandError["code"] = "other") => Promise.reject({ code, message });
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A Docker Compose start, as the launcher streams it. */
const LAUNCH_LINES = (slug: string) => [
  `Downloading ${slug}…`,
  "Lab installed",
  ` Image ${slug}-web Pulling`,
  ` Image ${slug}-web Pulled`,
  ` Network ${slug}_default Creating`,
  ` Network ${slug}_default Created`,
  ` Container ${slug}-web-1 Creating`,
  ` Container ${slug}-web-1 Created`,
  ` Container ${slug}-web-1 Starting`,
  ` Container ${slug}-web-1 Started`,
  ` Container ${slug}-web-1 Waiting`,
  ` Container ${slug}-web-1 Healthy`,
];

/** A started lab's status: one web container on its own network, next to the attack box. */
function startedStatus(id: string): LabStatus {
  if (id === "lab-invoice") return invoiceStatus;
  const lab = labs.find((l) => l.id === id);
  const name = lab?.slug ?? id;
  return {
    ...invoiceStatus,
    url: "http://127.0.0.1:8443",
    networks: [{ name: "default", subnet: "10.43.0.0/24", internal: false }],
    machines: [
      {
        name: "web",
        state: "running",
        image: `${name}:latest`,
        ip: "10.43.0.2",
        ports: [{ published: 8443, target: 443 }],
        interfaces: [{ network: "default", ip: "10.43.0.2" }],
        services: [{ name: "dashboard", kind: "web", ports: [443] }],
        infra: false,
      },
    ],
  };
}

const SELFTEST_STEPS: Record<string, string[]> = {
  docker: ["engine", "pull", "start", "network", "port", "cleanup"],
  vm: ["vagrant", "provider", "hypervisor", "box", "boot", "exec", "network", "cleanup"],
};

/** Servers and cloud accounts saved in the preview, shared by its tabs (the setup window saves,
 *  the main window lists): kept in the browser's storage, null until one is saved. */
type Host = Record<string, unknown> & { id?: string | null };
const HOSTS_KEY = "cyberctf.devMock.hosts";
function savedHosts(): Host[] | null {
  try {
    const raw = localStorage.getItem(HOSTS_KEY);
    return raw ? (JSON.parse(raw) as Host[]) : null;
  } catch {
    return null;
  }
}
function saveHosts(hosts: Host[]) {
  try {
    localStorage.setItem(HOSTS_KEY, JSON.stringify(hosts));
  } catch {
    /* storage unavailable: the save is not kept */
  }
}

/** A few of the core's checks on a saved server (runtime/server/input.rs), with its messages. */
function saveError(h: Host): string | null {
  const host = String(h.host ?? "").trim();
  const user = String(h.username ?? "").trim();
  if (h.provider === "proxmox" || h.provider === "vmware_esxi") {
    if (!/^[A-Za-z0-9.-]+$/.test(host)) return "host must be a hostname or IP address, without https:// or a path";
    if (h.provider === "proxmox" && !user.includes("@")) return "Proxmox users include a realm, e.g. root@pam";
  }
  if (h.provider === "aws" && !h.useCliCreds && !/^AKIA[A-Z0-9]{16}$/.test(user)) return "access key id looks wrong; an IAM user's key starts with AKIA...";
  return null;
}

/** The saved servers and cloud accounts: those saved in the preview, else the sample ones (without
 *  the cloud account under `cloud=none`). */
function hostsOf(sc: Scenario): Host[] {
  const sample = (answer("server_list", undefined) as { hosts: Host[] }).hosts;
  return savedHosts() ?? (sc.noCloud ? sample.filter((h) => h.provider !== "aws") : sample);
}

const labRuntime = (id: string) => labs.find((l) => l.id === id)?.runtime.runtime;

/** An operation on a lab: in flight (as the core reports it) while its lines stream. */
async function operation(id: string, op: NonNullable<ReturnType<Live["ops"]["get"]>>, lines: string[], args: Record<string, unknown> | undefined, live: Live) {
  live.ops.set(id, op);
  try {
    for (const line of lines) {
      send(args, line);
      await sleep(120);
    }
  } finally {
    live.ops.delete(id);
  }
}

/** A VM lab started with Vagrant (on this machine's hypervisor, or a saved server), or a cloud lab
 *  with Terraform in a saved cloud account. */
async function launchElsewhere(id: string, runtime: "VM" | "CLOUD", args: Record<string, unknown> | undefined, sc: Scenario, live: Live) {
  const slug = labs.find((l) => l.id === id)?.slug ?? id;
  const host = args?.host ? (hostsOf(sc).find((h) => h.id === args.host) ?? null) : null;
  if (args?.host && !host) return rejected(`no saved server or cloud account '${String(args.host)}'`);
  const provider = String(host?.provider ?? args?.provider ?? "virtualbox");
  const lines =
    runtime === "CLOUD"
      ? [
          `Downloading ${slug}…`,
          "Lab installed",
          "Initializing the backend...",
          "Initializing provider plugins...",
          "Terraform will perform the following actions:",
          "Plan: 6 to add, 0 to change, 0 to destroy.",
          "aws_instance.web: Creating...",
          "aws_instance.web: Still creating... [10s elapsed]",
          "aws_instance.web: Creation complete after 21s [id=i-0c0ffee5afe]",
          "Apply complete! Resources: 6 added, 0 changed, 0 destroyed.",
        ]
      : [
          `Downloading ${slug}…`,
          "Lab installed",
          ...(host ? [`Running on server host ${String(host.name)}`] : []),
          ...vagrantMachines().map((m) => `Bringing machine '${m}' up with '${provider}' provider...`),
          ...vagrantMachines().flatMap((m) => [
            `==> ${m}: Importing base box 'StefanScherer/windows_2019'...`,
            `==> ${m}: Booting VM...`,
            `==> ${m}: Machine booted and ready!`,
            `==> ${m}: Running provisioner: ansible...`,
          ]),
        ];
  if (live.launchFails) {
    live.launchFails = false;
    await operation(id, "launch", lines.slice(0, 4), args, live);
    return rejected(
      runtime === "CLOUD"
        ? "Error: creating EC2 Instance: UnauthorizedOperation: You are not authorized to perform this operation."
        : "The guest machine entered an invalid state while waiting for it to boot (while executing the action on the 'dc01' machine)",
    );
  }
  await operation(id, "launch", lines, args, live);
  live.placed.set(id, { provider, host, parked: null, startedAt: Date.now() });
  live.running.add(id);
  return null;
}

/** Events between the preview's tabs, as Tauri's event bus links the app's windows. */
type Internals = { runCallback: (id: number, data: unknown) => void };
const eventListeners = new Map<string, number[]>();
let eventBus: BroadcastChannel | null = null;
function deliver(event: string, payload: unknown) {
  const internals = (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__;
  for (const id of eventListeners.get(event) ?? []) internals.runCallback(id, { event, id, payload });
}

/** The browser preview's answers: the scenario's machine and the lab operations, then `answer`. */
export async function browserAnswer(cmd: string, args: Record<string, unknown> | undefined, sc: Scenario, live: Live): Promise<unknown> {
  switch (cmd) {
    case "plugin:event|listen": {
      const event = String(args?.event);
      const handler = Number(args?.handler);
      eventListeners.set(event, [...(eventListeners.get(event) ?? []), handler]);
      return handler;
    }
    case "plugin:event|unlisten": {
      const event = String(args?.event);
      eventListeners.set(
        event,
        (eventListeners.get(event) ?? []).filter((id) => id !== Number(args?.eventId)),
      );
      return null;
    }
    case "plugin:event|emit":
    case "plugin:event|emit_to": {
      const event = String(args?.event);
      deliver(event, args?.payload);
      eventBus?.postMessage({ event, payload: args?.payload });
      return null;
    }
    case "system_check": {
      // Decided when the answer is given: a check in flight when the machine comes back answers
      // like the next one, so no failure lands after the test's retry.
      if (live.checkDown) {
        await sleep(150);
        if (live.checkDown) return rejected("docker info timed out after 20 s");
      }
      const now = Date.now();
      if (!live.firstCheckAt) live.firstCheckAt = now;
      return withInstalls(scenarioReport(sc), live.installed, sc.pmLater ? now - live.firstCheckAt >= 1000 : null);
    }
    case "hyperv_join_admins":
      await sleep(300);
      return null;
    case "install_dependency":
    case "install_vagrant_plugin": {
      const what = String(cmd === "install_dependency" ? args?.dependency : args?.plugin);
      const lines = installLines(what);
      for (const line of lines.slice(0, -1)) {
        send(args, line);
        await sleep(150);
      }
      if (live.installFails) {
        live.installFails = false;
        return rejected(`${what}: the installer exited with code 1 (dev mock)`);
      }
      send(args, lines.at(-1));
      live.installed.add(what);
      return null;
    }
    case "auth_status":
      return live.signedIn ? answer(cmd, args) : { loggedIn: false, name: null, email: null };
    case "auth_login":
      // The link the "Copy the sign-in link" fallback offers.
      (args?.events as Stream | undefined)?.onmessage?.("https://www.cyberauth.co/api/auth/oauth2/authorize?client_id=dev-mock");
      await sleep(400);
      live.signedIn = true;
      return answer(cmd, args);
    case "agent_info":
      return { ...(answer(cmd, args) as object), arch: sc.arch };
    case "lab_status": {
      const id = String(args?.id);
      const placed = live.placed.get(id);
      if (placed) return labRuntime(id) === "CLOUD" ? cloudStatus(placed) : vmStatus(placed);
      return live.running.has(id) ? startedStatus(id) : stopped;
    }
    case "running_labs":
      return [...live.running];
    case "machine_workloads": {
      // The scan follows the labs started and stopped here: a started container lab shows up in
      // it (the sidebar lists running labs from it), a stopped one leaves it.
      const fixed = answer(cmd, args) as { id: string; kind: string }[];
      const kept = fixed.filter((w) => w.kind !== "docker" || live.running.has(w.id));
      const started = [...live.running].filter((id) => !fixed.some((w) => w.id === id) && !live.placed.has(id));
      return [...kept, ...started.map((id) => ({ id, kind: "docker", count: 1, memBytes: 0.3 * GB, provider: null }))];
    }
    case "deploying_labs":
      return [...new Set([...(sc.noDeploying ? [] : (answer(cmd, args) as string[])), ...live.ops.keys()])];
    case "active_operations":
      return [...(sc.noDeploying ? [] : (answer(cmd, args) as unknown[])), ...[...live.ops].map(([labId, op]) => ({ labId, op, machine: null, step: null }))];
    case "lab_launch": {
      const id = String(args?.labId);
      const runtime = labRuntime(id);
      if (runtime === "VM" || runtime === "CLOUD") return launchElsewhere(id, runtime, args, sc, live);
      const slug = labs.find((l) => l.id === id)?.slug ?? id;
      const lines = LAUNCH_LINES(slug);
      live.ops.set(id, "launch");
      try {
        for (const [n, line] of lines.entries()) {
          if (live.launchFails && n === 8) {
            live.launchFails = false;
            await sleep(120);
            return await rejected(
              `Error response from daemon: driver failed programming external connectivity on endpoint ${slug}-web-1: Bind for 0.0.0.0:8443 failed: port is already allocated`,
            );
          }
          send(args, line);
          // The containers run once started, before the launch is over (it waits until healthy).
          if (line.endsWith(" Started")) live.running.add(id);
          await sleep(120);
        }
        return null;
      } finally {
        live.ops.delete(id);
      }
    }
    case "lab_stop": {
      const id = String(args?.id);
      const slug = labs.find((l) => l.id === id)?.slug ?? id;
      const runtime = labRuntime(id);
      const lines =
        runtime === "VM"
          ? vagrantMachines().flatMap((m) => [`==> ${m}: Forcing shutdown of VM...`, `==> ${m}: Destroying VM and associated drives...`])
          : runtime === "CLOUD"
            ? [
                "aws_instance.web: Destroying... [id=i-0c0ffee5afe]",
                "aws_instance.web: Destruction complete after 31s",
                "Destroy complete! Resources: 6 destroyed.",
              ]
            : [` Container ${slug}-web-1 Stopping`, ` Container ${slug}-web-1 Stopped`, ` Container ${slug}-web-1 Removed`];
      await operation(id, "stop", lines, args, live);
      live.running.delete(id);
      live.placed.delete(id);
      return null;
    }
    case "lab_park": {
      const id = String(args?.id);
      const placed = live.placed.get(id);
      if (!placed) return rejected("the lab isn't running");
      const mode = args?.mode === "pause" ? "pause" : "shutdown";
      const verb = mode === "pause" ? "Saving VM state and suspending execution..." : "Attempting graceful shutdown of VM...";
      await operation(
        id,
        mode,
        vagrantMachines().map((m) => `==> ${m}: ${verb}`),
        args,
        live,
      );
      placed.parked = mode;
      live.running.delete(id);
      return null;
    }
    case "lab_resume": {
      const id = String(args?.id);
      const placed = live.placed.get(id);
      if (!placed?.parked) return rejected("the lab isn't parked");
      const back = placed.parked === "pause" ? "Resuming suspended VM..." : "Booting VM...";
      await operation(
        id,
        "resume",
        vagrantMachines().flatMap((m) => [`==> ${m}: ${back}`, `==> ${m}: Machine booted and ready!`]),
        args,
        live,
      );
      placed.parked = null;
      live.running.add(id);
      return null;
    }
    case "lab_provision": {
      const id = String(args?.id);
      if (!live.placed.get(id) || !live.running.has(id)) return rejected("the lab isn't running");
      const machines = vagrantMachines(args?.machine ? String(args.machine) : null);
      await operation(
        id,
        "provision",
        machines.flatMap((m) => [`==> ${m}: Running provisioner: ansible...`, `PLAY RECAP *********`, `${m} : ok=41 changed=3 unreachable=0 failed=0`]),
        args,
        live,
      );
      return null;
    }
    case "api_query": {
      const op = String(args?.operation ?? "");
      const vars = (args?.variables ?? {}) as Record<string, unknown>;
      if (op === "activeSession") return { myActiveLabSession: live.hosted };
      if (op === "launchHosted") {
        live.hosted = {
          id: `sess-${Date.now()}`,
          labId: String(vars.labId),
          target: "hosted",
          state: "REQUESTED",
          endpoints: [],
          message: null,
          expiresAt: new Date(Date.now() + 2 * 3600_000).toISOString(),
        };
        return { requestLabLaunch: live.hosted };
      }
      if (op === "labSession") {
        const h = live.hosted;
        if (!h || h.id !== vars.id) return { labSession: null };
        // One poll later it runs, with its public address.
        const slug = labs.find((l) => l.id === h.labId)?.slug ?? h.labId;
        live.hosted = { ...h, state: "RUNNING", endpoints: [{ port: 443, url: `https://${slug}-7f3a.labs.cyberctf.org` }] };
        return { labSession: live.hosted };
      }
      if (op === "stopSession") {
        live.hosted = null;
        return { stopLabSession: true };
      }
      return answer(cmd, args);
    }
    case "plugin:window|close":
      // The app closes its window; here, the tab (one the app opened, as its windows are).
      setTimeout(() => window.close(), 0);
      return null;
    case "plugin:deep-link|get_current":
      return sc.link ? [sc.link] : null;
    case "machine_selftest": {
      const channel = args?.events as Stream | undefined;
      for (const step of SELFTEST_STEPS[String(args?.kind)] ?? []) {
        channel?.onmessage?.({ step, state: "running", detail: null });
        await sleep(60);
        channel?.onmessage?.({ step, state: "ok", detail: null });
      }
      return null;
    }
    // A stand-in shell for the attack box: a prompt, an echo, and a few commands' answers.
    case "terminal_open": {
      const channel = args?.events as Stream | undefined;
      const session = ++terminalSessions;
      terminals.set(session, { channel, line: "" });
      channel?.onmessage?.({ kind: "data", data: "Connected to the attack box (dev mock).\r\n" + PROMPT });
      return session;
    }
    case "terminal_write": {
      const t = terminals.get(Number(args?.session));
      if (!t) return rejected("the shell has ended");
      for (const ch of String(args?.data ?? "")) {
        if (ch === "\r" && t.line.trim() === "exit") {
          terminals.delete(Number(args?.session));
          t.channel?.onmessage?.({ kind: "data", data: "\r\nlogout\r\n" });
          t.channel?.onmessage?.({ kind: "exit", code: 0 });
          return null;
        } else if (ch === "\r") {
          const reply = MOCK_SHELL[t.line.trim()] ?? (t.line.trim() ? `${t.line.trim().split(" ")[0]}: command not found (dev mock)` : "");
          t.channel?.onmessage?.({ kind: "data", data: `\r\n${reply ? `${reply}\r\n` : ""}${PROMPT}` });
          t.line = "";
        } else if (ch === "\x7f") {
          if (t.line) t.channel?.onmessage?.({ kind: "data", data: "\b \b" });
          t.line = t.line.slice(0, -1);
        } else {
          t.line += ch;
          t.channel?.onmessage?.({ kind: "data", data: ch });
        }
      }
      return null;
    }
    case "terminal_resize":
      return null;
    case "terminal_close": {
      const t = terminals.get(Number(args?.session));
      terminals.delete(Number(args?.session));
      t?.channel?.onmessage?.({ kind: "exit", code: 0 });
      return null;
    }
    case "machine_open_setup": {
      const step = args?.step ? `step=${String(args.step)}` : "";
      openWindow(`/machine-setup${step ? `?${step}` : ""}`);
      return null;
    }
    case "terminal_window": {
      const q = new URLSearchParams({ id: String(args?.id), kind: String(args?.kind), runtime: String(args?.runtime) });
      openWindow(`/shell?${q}`);
      return null;
    }
    case "server_list": {
      const list = answer(cmd, args) as { default: string; hosts: Host[] };
      return { ...list, hosts: hostsOf(sc) };
    }
    case "server_save": {
      const input = (args?.input ?? {}) as Host;
      const invalid = saveError(input);
      if (invalid) return rejected(invalid);
      const saved = { ...input, id: input.id ?? `host-${Date.now()}`, password: undefined };
      const hosts = hostsOf(sc);
      saveHosts([...hosts.filter((h) => h.id !== saved.id), saved]);
      return saved;
    }
    default:
      return answer(cmd, args);
  }
}

const PROMPT = "\x1b[1;31mroot@attack-box\x1b[0m:~# ";
/** What the mock shell answers. */
const MOCK_SHELL: Record<string, string> = {
  id: "uid=0(root) gid=0(root) groups=0(root)",
  whoami: "root",
  hostname: "attack-box",
  "ip -br a": "lo      UNKNOWN 127.0.0.1/8\r\neth0    UP      10.20.0.2/24",
};
let terminalSessions = 0;
const terminals = new Map<number, { channel: Stream | undefined; line: string }>();

/** Installs the mock when running in a plain browser with ?mock in the URL. */
export function installDevMock() {
  if (process.env.NODE_ENV === "production" || typeof window === "undefined") return;
  if ("__TAURI_INTERNALS__" in window && !(window as unknown as { __CYBERCTF_MOCK__?: boolean }).__CYBERCTF_MOCK__) return;
  if (!new URLSearchParams(window.location.search).has("mock")) return;
  const sc = scenarioFrom(window.location.search);
  const live: Live = {
    running: new Set(answer("running_labs", undefined) as string[]),
    signedIn: !sc.signedOut,
    checkDown: sc.checkDown,
    launchFails: sc.launchFails,
    ops: new Map(),
    installed: new Set(),
    installFails: sc.installFails,
    firstCheckAt: 0,
    placed: new Map(),
    hosted: null,
  };
  mockWindows("main");
  mockIPC((cmd, args) => browserAnswer(cmd, args as Record<string, unknown> | undefined, sc, live));
  if (typeof BroadcastChannel !== "undefined" && !eventBus) {
    eventBus = new BroadcastChannel("cyberctf-dev-mock-events");
    eventBus.onmessage = (e: MessageEvent<{ event: string; payload: unknown }>) => deliver(e.data.event, e.data.payload);
  }
  (window as unknown as { __CYBERCTF_MOCK__?: boolean }).__CYBERCTF_MOCK__ = true;
  (window as unknown as { __CYBERCTF_MOCK_MACHINE_UP__?: () => void }).__CYBERCTF_MOCK_MACHINE_UP__ = () => {
    live.checkDown = false;
  };
  if (sc.onboarding) clearOnboarded();
  else markOnboarded();
}
