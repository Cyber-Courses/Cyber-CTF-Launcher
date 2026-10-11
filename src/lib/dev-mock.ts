/**
 * Browser preview of the UI (development only): `pnpm dev`, then open
 * http://localhost:3000/?mock. Outside Tauri there is no Rust core, so this answers the
 * commands the screens call with plausible sample data (a signed-in player, two running labs,
 * a server, an AWS account); Settings and the server setup open in new tabs. Never loaded in
 * a production build or inside the app.
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const GB = 1e9;

const labs = [
  {
    id: "lab-invoice",
    slug: "invoice-portal-api",
    title: "Invoice portal API",
    description: "A billing portal exposes a REST API guarded by a static key. Read what the key unlocks.",
    question: "What is the total amount of invoice INV-2041?",
    difficulty: 1,
    category: "Web",
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["docker", "aws"], hosted: false },
    skills: [
      { id: "s1", name: "API keys" },
      { id: "s2", name: "IDOR" },
    ],
  },
  {
    id: "lab-goad",
    slug: "goad-light",
    title: "GOAD Light",
    description: "Two domains, one forest, one weak trust.",
    question: "Which account holds the NTLM hash you recovered?",
    difficulty: 3,
    category: "Active Directory",
    runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "vmware_esxi", "proxmox"], hosted: false },
    skills: [{ id: "s3", name: "Kerberoasting" }],
  },
  {
    id: "lab-sqli",
    slug: "sqli-blind-orders",
    title: "Blind orders",
    description: "Pull a customer's email through blind SQL injection.",
    question: null,
    difficulty: 2,
    category: "Web",
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["docker"], hosted: true },
    skills: [{ id: "s4", name: "SQL injection" }],
  },
  {
    id: "lab-kube",
    slug: "kube-dashboard-exposed",
    title: "Exposed dashboard",
    description: "From an open dashboard to cluster secrets.",
    question: null,
    difficulty: 2,
    category: "Containers",
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["docker"], hosted: false },
    skills: [],
  },
  {
    id: "lab-s3",
    slug: "s3-public-backup",
    title: "Public backup",
    description: "A forgotten bucket with a payroll export.",
    question: null,
    difficulty: 1,
    category: "Cloud",
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["aws"], hosted: false },
    skills: [],
  },
];

const invoiceStatus = {
  running: true,
  parked: null,
  url: "http://127.0.0.1:8080",
  host: null,
  expiresAt: null,
  place: "container",
  provider: null,
  networks: [{ name: "default", subnet: "10.42.0.0/24", internal: false }],
  machines: [
    {
      name: "api",
      state: "running",
      image: "node:20",
      ip: "10.42.0.3",
      ports: [
        { published: 8080, target: 8080 },
        { published: 9090, target: 9090 },
      ],
      interfaces: [{ network: "default", ip: "10.42.0.3" }],
      services: [
        { name: "invoice-api", kind: "web", ports: [8080] },
        { name: "admin", kind: "web", ports: [9090] },
      ],
      infra: false,
    },
    {
      name: "postgres",
      state: "running",
      image: "postgres:16",
      ip: "10.42.0.4",
      ports: [{ published: 5432, target: 5432 }],
      interfaces: [{ network: "default", ip: "10.42.0.4" }],
      services: [{ name: "postgres", kind: "database", ports: [5432] }],
      infra: false,
    },
    {
      name: "cache",
      state: "running",
      image: "redis:7",
      ip: "10.42.0.5",
      ports: [],
      interfaces: [{ network: "default", ip: "10.42.0.5" }],
      services: [],
      infra: false,
    },
  ],
};
const stopped = { running: false, parked: null, url: null, host: null, expiresAt: null, place: null, provider: null, networks: [], machines: [] };

const report = {
  os: "macos",
  arch: "arm64",
  pkgManager: { name: "brew", installed: true, version: "4.4.0" },
  docker: { installed: true, version: "27.3.1" },
  dockerRunning: true,
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
};

let cpu = 38;
let sharedFolder = { enabled: false, path: "/Users/florian/cyberctf-share", mountPoint: "/workspace/share" };

/** The app opens Settings and the server setup in their own windows; here, a browser tab. */
function openWindow(path: string) {
  window.open(`${path}${path.includes("?") ? "&" : "?"}mock`, "_blank");
}

export function answer(cmd: string, args: Record<string, unknown> | undefined): unknown {
  switch (cmd) {
    case "system_check":
      return report;
    case "auth_status":
    case "auth_login":
      return { loggedIn: true, name: "Florian Amette", email: "florian@cyberctf.org" };
    case "agent_info":
      return { installId: "c0ffee", name: "florian-mbp", arch: "arm64", capabilities: ["docker", "vagrant"] };
    case "api_query": {
      const q = String(args?.query ?? "");
      if (q.includes("myCompletedLabs")) return { myCompletedLabs: ["lab-s3"] };
      if (q.includes("labs")) return { labs };
      return {};
    }
    case "lab_status":
      return args?.id === "lab-invoice" ? invoiceStatus : stopped;
    case "running_labs":
      return ["lab-invoice"];
    case "deploying_labs":
      return ["lab-goad"];
    case "active_operations":
      return [{ labId: "lab-goad", op: "launch", machine: "SRV02", step: "Configure SRV02 (ansible 41/88)" }];
    case "lab_deploy_log":
      return "";
    case "machine_metrics":
      cpu = Math.max(8, Math.min(92, cpu + (Math.random() - 0.5) * 14));
      return { cpu, memUsed: 19.5 * GB, memTotal: 32 * GB, diskUsed: 370 * GB, diskTotal: 500 * GB, uptimeSecs: 86400 * 3, cores: 8, containers: 6 };
    case "machine_workloads":
      return [
        { id: "lab-invoice", kind: "docker", count: 3, memBytes: 0.6 * GB, provider: null },
        { id: "lab-goad", kind: "vm", count: 3, memBytes: 12 * GB, provider: "virtualbox" },
      ];
    case "machine_storage":
      return {
        images: [
          { name: "cyberctf/attack-box:latest", bytes: 4.1 * GB },
          { name: "postgres:16", bytes: 0.43 * GB },
        ],
        boxes: [{ name: "StefanScherer/windows_2019", bytes: 11.2 * GB }],
      };
    case "server_list":
      return {
        default: "pve",
        hosts: [
          {
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
          },
          {
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
          },
          {
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
          },
        ],
      };
    case "server_capacity":
      return { cores: 32, memTotal: 64 * GB, memFree: 42 * GB };
    case "server_test":
      return { ok: true, reachable: true, authenticated: true, latencyMs: 12, message: "Connected", checks: [] };
    case "aws_month_to_date_cost":
      return 6.4;
    case "aws_cli_identity":
      return "arn:aws:iam::482100000193:user/florian";
    case "aws_profiles":
      return ["default"];
    case "exegol_status":
    case "attack_vm_status":
      return { imagePresent: true, running: true, ip: "10.42.0.9", labNetwork: "default", shellCmd: "docker exec -it attack-box zsh" };
    case "lab_tools":
      return [{ name: "Burp Suite", image: null, addresses: [{ network: "default", ip: "10.42.0.9" }], publish: null }];
    case "lab_check":
      return { available: true, ok: true, output: "", results: [{ name: "api", from: "attacker", ok: true, reason: "" }] };
    case "image_download_size":
      return 4.1 * GB;
    case "installed_tools":
      return [];
    case "shared_folder_get":
      return sharedFolder;
    case "shared_folder_set":
      sharedFolder = { ...sharedFolder, enabled: !!args?.enabled, path: String(args?.path ?? sharedFolder.path) };
      return sharedFolder;
    case "open_settings":
      openWindow("/?window=settings");
      return null;
    case "server_open_setup": {
      const id = args?.id ? `?id=${encodeURIComponent(String(args.id))}` : args?.kind === "cloud" ? "?kind=cloud" : "";
      openWindow(`/server-setup${id}`);
      return null;
    }
    case "plugin:app|version":
      return "0.3.2";
    case "plugin:notification|is_permission_granted":
      return false;
    case "plugin:event|listen":
      return 1;
    default:
      return null;
  }
}

/** Installs the mock when running in a plain browser with ?mock in the URL. */
export function installDevMock() {
  if (process.env.NODE_ENV === "production" || typeof window === "undefined") return;
  if ("__TAURI_INTERNALS__" in window && !(window as unknown as { __CYBERCTF_MOCK__?: boolean }).__CYBERCTF_MOCK__) return;
  if (!new URLSearchParams(window.location.search).has("mock")) return;
  mockWindows("main");
  mockIPC((cmd, args) => answer(cmd, args as Record<string, unknown> | undefined));
  (window as unknown as { __CYBERCTF_MOCK__?: boolean }).__CYBERCTF_MOCK__ = true;
  try {
    localStorage.setItem("cyberctf.onboarded", "1");
  } catch {
    /* ignore */
  }
}
