import type { ImportedLab, LabRunOptions, LabRunPath, LabStatus, Provider } from "@/lib/tauri";
import { argText } from "@/lib/dev-mock/command";
import { objectivesAnswer } from "@/lib/dev-mock/objectives";
import type { Args, Handlers } from "@/lib/dev-mock/types";

// The dev mock's labs (see index.ts): the catalogue and their statuses, answered as the backend
// would. The objectives of "Exposed dashboard" are in objectives.ts.

/** A catalogue lab's source, as the catalogue lists it: its repository and the commit it points
 *  at, where the backend reads the lab's spec (its run options, where it starts by default). */
const source = (slug: string) => ({ repository: `CyberCTF/${slug}`, commit: "4c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d" });

export const labs = [
  {
    id: "lab-invoice",
    slug: "invoice-portal-api",
    title: "Invoice portal API",
    description: "A billing portal exposes a REST API guarded by a static key. Read what the key unlocks.",
    question: "What is the total amount of invoice INV-2041?",
    difficulty: 1,
    category: "Web",
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["docker", "aws"], ...source("invoice-portal-api") },
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
    runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "vmware_esxi", "proxmox"], ...source("goad-light") },
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
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["docker"], ...source("sqli-blind-orders") },
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
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["docker"], ...source("kube-dashboard-exposed") },
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
    runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["aws"], ...source("s3-public-backup") },
    skills: [],
  },
  {
    id: "lab-goat",
    slug: "cloudgoat-ec2-ssrf",
    title: "EC2 SSRF",
    description: "From a low-privilege IAM user to a Lambda function, through an SSRF on EC2.",
    question: null,
    difficulty: 2,
    category: "Cloud",
    runtime: {
      runtime: "CLOUD",
      architectures: [],
      providers: ["aws"],
      repository: "CyberCTF/cloudgoat-ec2-ssrf",
      commit: "0123456789abcdef0123456789abcdef01234567",
    },
    skills: [{ id: "s5", name: "IAM" }],
  },
];

/** Labs the catalogue lists only under a switch (scenario.ts): a VM lab built for arm64 too, so
 *  an Apple Silicon Mac can run one on UTM (`catalogue=arm64vm`); GOAD Light is x86 only. */
export const extraLabs = [
  {
    id: "lab-router",
    slug: "edge-router",
    title: "Edge router",
    description: "A branch office router with a management interface left on the WAN side.",
    question: null,
    difficulty: 2,
    category: "Network",
    runtime: { runtime: "VM", architectures: ["arm64", "x86_64"], providers: ["utm", "virtualbox", "vmware_esxi", "proxmox"], ...source("edge-router") },
    skills: [{ id: "s6", name: "Routing" }],
  },
];

// A wide topology on purpose (three networks, two pivots), so the lab page's diagram can be
// checked at every window width: the public network, then the billing back end behind the API,
// then the finance network behind the billing worker.
export const invoiceStatus: LabStatus = {
  running: true,
  parked: null,
  url: "http://127.0.0.1:8080",
  host: null,
  hostId: null,
  expiresAt: null,
  place: "container",
  provider: null,
  perMachine: false,
  outputs: [],
  message: null,
  reapError: null,
  controls: ["shutdown"],
  networks: [
    { name: "default", subnet: "10.42.0.0/24", internal: false },
    { name: "billing", subnet: "10.42.1.0/24", internal: true },
    { name: "finance", subnet: "10.42.2.0/24", internal: true },
  ],
  machines: [
    {
      name: "api",
      state: "running",
      ip: "10.42.0.3",
      ports: [
        { published: 8080, target: 8080 },
        { published: 9090, target: 9090 },
      ],
      interfaces: [
        { network: "default", ip: "10.42.0.3" },
        { network: "billing", ip: "10.42.1.3" },
      ],
      services: [
        { name: "invoice-api", kind: "web", ports: [8080] },
        { name: "admin", kind: "web", ports: [9090] },
      ],
      infra: false,
      console: null,
    },
    {
      name: "postgres",
      state: "running",
      ip: "10.42.0.4",
      ports: [{ published: 5432, target: 5432 }],
      interfaces: [{ network: "default", ip: "10.42.0.4" }],
      services: [{ name: "postgres", kind: "database", ports: [5432] }],
      infra: false,
      console: null,
    },
    {
      name: "cache",
      state: "running",
      ip: "10.42.0.5",
      ports: [],
      interfaces: [{ network: "default", ip: "10.42.0.5" }],
      services: [],
      infra: false,
      console: null,
    },
    {
      name: "billing-worker",
      state: "running",
      ip: "10.42.1.10",
      ports: [],
      interfaces: [
        { network: "billing", ip: "10.42.1.10" },
        { network: "finance", ip: "10.42.2.10" },
      ],
      services: [{ name: "invoice-jobs", kind: "worker", ports: [] }],
      infra: false,
      console: null,
    },
    {
      name: "ledger-db",
      state: "running",
      ip: "10.42.1.11",
      ports: [],
      interfaces: [{ network: "billing", ip: "10.42.1.11" }],
      services: [{ name: "ledger", kind: "database", ports: [3306] }],
      infra: false,
      console: null,
    },
    {
      name: "payroll",
      state: "running",
      ip: "10.42.2.20",
      ports: [],
      interfaces: [{ network: "finance", ip: "10.42.2.20" }],
      services: [{ name: "payroll-web", kind: "web", ports: [80] }],
      infra: false,
      console: null,
    },
    {
      name: "backup",
      state: "running",
      ip: "10.42.2.21",
      ports: [],
      interfaces: [{ network: "finance", ip: "10.42.2.21" }],
      services: [{ name: "sftp", kind: "ssh", ports: [22] }],
      infra: false,
      console: null,
    },
  ],
};
export const stopped: LabStatus = {
  running: false,
  parked: null,
  url: null,
  host: null,
  hostId: null,
  expiresAt: null,
  place: null,
  provider: null,
  perMachine: false,
  reapError: null,
  controls: [],
  networks: [],
  machines: [],
  outputs: [],
  message: null,
};

/** The lab imported from a folder the preview's `imported` switch installs: what its `.ctf/`
 *  folder says of it. */
export const importedSample: ImportedLab = {
  id: "local-supplier-portal",
  name: "supplier-portal",
  metadata: {
    title: "Supplier portal",
    description: "An unpublished lab, imported from its folder: a supplier API that trusts its invoice numbers.",
    category: "web",
    difficulty: 1,
    capabilities: ["IDOR"],
  },
  objectives: null,
  problems: [],
};

/** The lab commands: their status, a cloud lab's preview, what runs and deploys. */
export const labCommands = {
  lab_status: (args) => (args?.id === "lab-invoice" ? invoiceStatus : stopped),
  lab_cloud_preview: (args) =>
    args?.id === "lab-goat" ? { provider: "aws", hourlyUsd: 0.02, inputs: ["PLAYER_CIDR"], allowList: true, outputs: ["access_key_id", "secret_key"] } : null,
  lab_requirements: (args) =>
    args?.id === "lab-goad"
      ? { min: { cpus: 4, memoryMb: 2560, diskGb: 25 }, max: { cpus: 6, memoryMb: 12288, diskGb: 120 } }
      : { min: { cpus: 2, memoryMb: 1024, diskGb: 20 }, max: { cpus: 2, memoryMb: 1024, diskGb: 20 } },
  // The sample's default server is Proxmox, which runs the VM labs; the others start here.
  lab_default_place: (args) => ({
    host: args?.runtime === "VM" ? "pve" : null,
    provider: null,
    perMachineVm: false,
    hypervisor: args?.runtime === "VM" ? "virtualbox" : null,
  }),
  lab_run_options: (args) => runOptions(args?.runtime),
  running_labs: () => ["lab-invoice"],
  busy_labs: () => ["lab-goad"],
  active_operations: () => [{ labId: "lab-goad", op: "launch", machine: "SRV02", deploying: true, step: "Configure SRV02 (ansible 41/88)" }],
  lab_deploy_log: () => "",
  lab_tools: () => [{ name: "Burp Suite", image: null, addresses: [{ network: "default", ip: "10.42.0.9" }], publish: null }],
  // No lab imported from a folder in the sample (the preview's `imported` switch adds one).
  imported_labs: () => [],
  // The running sample lab is the one installed here.
  lab_installed: (args) => args?.id === "lab-invoice",
  lab_remove: () => ({ left: [] }),
  lab_check: () => ({ available: true, ok: true, output: "", results: [{ name: "api", from: "attacker", ok: true, reason: "" }] }),
} satisfies Handlers;

/** The labs' briefs (`labBrief`), in markdown; a lab not listed has none. */
const BRIEFS: Record<string, string> = {
  "lab-sqli":
    "The shop's order lookup answers only *found* or *not found*. Ask it the right questions, one character at a time, and it gives up a customer's email.",
  "lab-invoice": "The billing API trusts one static key for every customer. Find it, then read invoices that aren't yours.",
};

/** A GraphQL operation (`api_query`): the completed labs, the objectives, the catalogue, a brief. */
export function apiQuery(args: Args): unknown {
  const op = argText(args?.operation);
  if (op === "completedLabs") return { myCompletedLabs: ["lab-s3"] };
  if (["labObjectives", "labProgress", "submitObjective", "openObjectiveHelp"].includes(op))
    return objectivesAnswer(op, (args?.variables ?? {}) as Record<string, unknown>);
  if (["labs", "labTitles", "sidebarLabs"].includes(op)) return { labs };
  if (op === "labBrief") return { labs: [{ contentMd: BRIEFS[argText((args?.variables as Record<string, unknown> | undefined)?.id)] ?? null }] };
  return {};
}

/** A badge-less path, or a Beta one. */
const runPath = (provider: Provider | null, beta = false): LabRunPath => ({
  provider,
  badge: beta ? "beta" : "none",
  evidence: beta ? "validated only; tier 1 pending: linux, macos, windows" : "real runs: macos (mac, 2026-10-17)",
});

/** UTM's path for a VM lab whose spec has a recipe for it. */
export const utmPath = runPath("utm", true);

const CLOUDS_OF_SAMPLE: Provider[] = ["aws", "azure", "gcp", "digitalocean", "linode", "oci"];
const NO_OPTIONS: LabRunOptions = {
  docker: null,
  dockerVm: [],
  vms: [],
  servers: [],
  cloudDocker: [],
  cloudVm: [],
  cloudServices: [],
  refused: [],
  emulated: false,
};

/** Where the sample's labs run, as the backend reads it from their spec: a container lab on
 *  Docker, in a helper VM and on a server or a cloud VM (none has VM recipes too), a VM lab as
 *  VMs (not on UTM), cloud services on AWS. */
export function runOptions(runtime: unknown): LabRunOptions {
  const clouds = () => CLOUDS_OF_SAMPLE.map((c) => runPath(c, c !== "aws"));
  if (runtime === "CLOUD") return { ...NO_OPTIONS, cloudServices: [runPath("aws")] };
  const local: Provider[] = ["virtualbox", "vmware_desktop", "parallels", "libvirt", "qemu"];
  const utm = { provider: "utm" as const, cause: { kind: "isoloom" as const, reason: "a VM on UTM gets no private network" } };
  if (runtime === "VM")
    return {
      ...NO_OPTIONS,
      vms: [...local, "hyperv" as const].map((p) => runPath(p, p !== "virtualbox")),
      servers: [runPath("vmware_esxi"), runPath("proxmox")],
      cloudVm: clouds(),
      refused: [utm],
    };
  return {
    ...NO_OPTIONS,
    docker: runPath(null),
    dockerVm: [...local, "utm" as const].map((p) => runPath(p, p !== "virtualbox")),
    servers: [runPath("vmware_esxi", true), runPath("proxmox", true)],
    cloudDocker: clouds(),
  };
}
