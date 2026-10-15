// The dev mock's sample data (see index.ts): a signed-in player, two running labs, a server and
// an AWS account, as the core answers each command. The component tests build on it; the
// browser preview's scenarios (browser.ts) change it as the preview is used.
import { GB } from "@/lib/format";
import type { ServerHost, ServerList, SystemReport } from "@/lib/tauri";
import { argText } from "@/lib/dev-mock/command";
import { apiQuery, labCommands } from "@/lib/dev-mock/labs";
import type { Args, Handler, Handlers } from "@/lib/dev-mock/types";
import { openWindow } from "@/lib/dev-mock/windows";

export const report: SystemReport = {
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
    sharedFolder = { ...sharedFolder, enabled: !!args?.enabled, path: argText(args?.path, sharedFolder.path) };
    return sharedFolder;
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
    const id = args?.id ? `?id=${encodeURIComponent(argText(args.id))}` : args?.kind === "cloud" ? "?kind=cloud" : "";
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
