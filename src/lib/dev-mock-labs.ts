// The dev mock's labs (see dev-mock.ts): the catalogue, their statuses, and the objectives of
// "Exposed dashboard" with the player's progress, answered as the backend would.

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
      hosted: false,
      repository: "CyberCTF/cloudgoat-ec2-ssrf",
      commit: "0123456789abcdef0123456789abcdef01234567",
    },
    skills: [{ id: "s5", name: "IAM" }],
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
const stopped = {
  running: false,
  parked: null,
  url: null,
  host: null,
  expiresAt: null,
  place: null,
  provider: null,
  networks: [],
  machines: [],
  outputs: [],
  message: null,
};

// Objectives (docs/lab-objectives.md in CyberBackend) for the preview: "Exposed dashboard" has
// three challenges, one optional, with guided steps and hints; every other lab keeps its
// implicit main. Answers here are sample values, checked in the browser only.
const kubeObjectives = [
  {
    key: "find-dashboard",
    title: "Find the dashboard",
    prompt: "Which port does the Kubernetes dashboard listen on inside the cluster?",
    optional: false,
    points: null,
    hintCount: 2,
    steps: [],
  },
  {
    key: "read-secret",
    title: "Read the database secret",
    prompt: "What is the password stored in the db-credentials secret?",
    optional: false,
    points: null,
    hintCount: 1,
    steps: [
      { key: "service-account", title: "Find the service account", prompt: "Which service account does the dashboard run as?", hintCount: 1 },
      { key: "namespace", title: "Find the namespace", prompt: "Which namespace holds db-credentials?", hintCount: 0 },
    ],
  },
  {
    key: "node-escape",
    title: "Reach the node",
    prompt: "Read /etc/hostname on the node the dashboard runs on.",
    optional: true,
    points: null,
    hintCount: 0,
    steps: [],
  },
];
const kubeAnswers: Record<string, string> = {
  "find-dashboard": "8443",
  "read-secret": "s3cr3t-db",
  "service-account": "kubernetes-dashboard",
  namespace: "prod",
  "node-escape": "worker-1",
  main: "1337.00",
};
const kubeHints: Record<string, string[]> = {
  "find-dashboard": ["List the services in every namespace.", "The dashboard's service maps 443 to its container port."],
  "read-secret": ["The dashboard's token can read secrets in one namespace."],
  "service-account": ["Look at the dashboard deployment's spec."],
};
const objectiveProgress = { solved: [] as string[], stepsOpened: [] as string[], hints: [] as { key: string; index: number; text: string }[] };

function mockProgress(labId: string) {
  const challenges = labId === "lab-kube" ? kubeObjectives : [{ key: "main", optional: false }];
  const required = challenges.filter((c) => !c.optional);
  const requiredSolved = required.filter((c) => objectiveProgress.solved.includes(c.key)).length;
  return { ...objectiveProgress, requiredSolved, requiredTotal: required.length, completed: requiredSolved === required.length };
}

export function mockObjectives(op: string, v: Record<string, unknown>): unknown {
  const labId = String(v.id ?? v.labId ?? "");
  if (op === "submitObjective") {
    const key = String(v.key);
    const before = mockProgress(labId).completed;
    const alreadySolved = objectiveProgress.solved.includes(key);
    const correct = String(v.answer).trim().toLowerCase() === kubeAnswers[key];
    if (correct && !alreadySolved) objectiveProgress.solved.push(key);
    return { submitObjective: { correct, alreadySolved, labCompleted: before || mockProgress(labId).completed } };
  }
  if (op === "openObjectiveHelp") {
    const key = String(v.key);
    if (v.what === "STEPS") {
      if (!objectiveProgress.stepsOpened.includes(key)) objectiveProgress.stepsOpened.push(key);
      return { openObjectiveHelp: { ok: true, hint: null } };
    }
    const index = Number(v.index);
    const text = kubeHints[key]?.[index] ?? "";
    if (!objectiveProgress.hints.some((h) => h.key === key && h.index === index)) objectiveProgress.hints.push({ key, index, text });
    return { openObjectiveHelp: { ok: true, hint: text } };
  }
  if (op === "labProgress") return { labs: [{ id: labId, myProgress: mockProgress(labId) }] };
  return { labs: [{ id: labId, objectives: labId === "lab-kube" ? kubeObjectives : [] }] };
}

/** The lab commands: their status, a cloud lab's preview, what runs and deploys. */
export const labCommands: Record<string, (args: Record<string, unknown> | undefined) => unknown> = {
  lab_status: (args) => (args?.id === "lab-invoice" ? invoiceStatus : stopped),
  lab_cloud_preview: (args) =>
    args?.id === "lab-goat" ? { provider: "aws", hourlyUsd: 0.02, inputs: ["PLAYER_CIDR"], allowList: true, outputs: ["access_key_id", "secret_key"] } : null,
  running_labs: () => ["lab-invoice"],
  deploying_labs: () => ["lab-goad"],
  active_operations: () => [{ labId: "lab-goad", op: "launch", machine: "SRV02", step: "Configure SRV02 (ansible 41/88)" }],
  lab_deploy_log: () => "",
  lab_tools: () => [{ name: "Burp Suite", image: null, addresses: [{ network: "default", ip: "10.42.0.9" }], publish: null }],
  lab_check: () => ({ available: true, ok: true, output: "", results: [{ name: "api", from: "attacker", ok: true, reason: "" }] }),
};

/** A GraphQL operation (`api_query`): the completed labs, the objectives, the catalogue. */
export function apiQuery(args: Record<string, unknown> | undefined): unknown {
  const op = String(args?.operation ?? "");
  if (op === "completedLabs") return { myCompletedLabs: ["lab-s3"] };
  if (["labObjectives", "labProgress", "submitObjective", "openObjectiveHelp"].includes(op))
    return mockObjectives(op, (args?.variables ?? {}) as Record<string, unknown>);
  if (["labs", "labTitles", "sidebarLabs"].includes(op)) return { labs };
  return {};
}
