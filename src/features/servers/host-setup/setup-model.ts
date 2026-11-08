import type {
  CloudClis,
  CloudProvider,
  Dependency,
  ProxmoxPlacement,
  RemoteProvider,
  ServerHostInput,
  ServerTest,
  SshKeySignIn,
  SystemReport,
} from "@/lib/tauri";
import {
  AWS_REGIONS,
  AZURE_LOCATIONS,
  CLOUD_DEFAULT_NAME,
  CLOUD_META,
  DO_REGIONS,
  EMPTY_CLOUD,
  EMPTY_HOST,
  GCP_REGIONS,
  KIND,
  LINODE_REGIONS,
  OCI_REGIONS,
  type StepKey,
} from "@/features/servers/host-setup/constants";

// The server / cloud setup's logic, as pure functions of the form and what this machine has.

const CLOUDS: readonly string[] = Object.keys(CLOUD_DEFAULT_NAME);
const DEFAULT_NAMES = Object.values(CLOUD_DEFAULT_NAME);

export const isCloud = (p: RemoteProvider): p is CloudProvider => CLOUDS.includes(p);

/** Token clouds (DigitalOcean, Linode) authenticate with just a pasted API token (no CLI, no username). */
const isTokenCloud = (p: RemoteProvider) => p === "digitalocean" || p === "linode";

/** Azure and GCP authenticate through their CLI (no access keys); the flow is the same shape. */
const isCliAuthCloud = (p: RemoteProvider) => p === "azure" || p === "gcp";

/**
 * The ordered steps for this setup. Editing skips choosing the provider or hypervisor and
 * installing the tools; a cloud setup opened on a chosen provider (`providerChosen`, the Cloud
 * page's Connect rows) skips choosing it too; only AWS asks how to connect (CLI or access keys).
 * Proxmox describes the server first, then picks how to sign in to it.
 */
export function setupSteps(provider: RemoteProvider, editing: boolean, providerChosen = false): StepKey[] {
  if (!isCloud(provider)) {
    const connect: StepKey[] = provider === "proxmox" ? ["server", "connection"] : ["connection"];
    return editing ? [...connect, "placement", "test"] : ["hypervisor", "tools", ...connect, "placement", "test"];
  }
  const account: StepKey[] = provider === "aws" ? ["account"] : [];
  const choose: StepKey[] = editing ? [] : providerChosen ? ["tools"] : ["provider", "tools"];
  return [...choose, ...account, "credentials", "options", "test"];
}

/** Where a new account's labs run by default, per cloud. */
export const DEFAULT_REGION: Record<CloudProvider, string> = {
  aws: "eu-west-3",
  azure: "swedencentral",
  gcp: "europe-west1",
  digitalocean: "fra1",
  linode: "eu-central",
  oci: "eu-frankfurt-1",
};

/** The region / location choices for a cloud (code, human name). */
export const REGIONS: Record<CloudProvider, [string, string][]> = {
  aws: AWS_REGIONS,
  azure: AZURE_LOCATIONS,
  gcp: GCP_REGIONS,
  digitalocean: DO_REGIONS,
  linode: LINODE_REGIONS,
  oci: OCI_REGIONS,
};

/** The form after choosing cloud `id`: its region and fresh credentials, its short name unless
 *  the user typed their own. */
export function withProvider(s: ServerHostInput, id: CloudProvider): ServerHostInput {
  return {
    ...s,
    provider: id,
    name: !s.name.trim() || DEFAULT_NAMES.includes(s.name.trim()) ? CLOUD_DEFAULT_NAME[id] : s.name,
    host: DEFAULT_REGION[id],
    username: "",
    password: null,
    useCliCreds: id === "aws",
    awsProfile: null,
    // node = GCP org id; clear it when switching provider.
    node: null,
    // Budget is AWS-only; don't carry one typed on AWS over to Azure/GCP.
    monthlyLimit: id === "aws" ? s.monthlyLimit : null,
  };
}

/** What a new setup starts from, as its window's query names it: a server, a new cloud account,
 *  or one on the cloud `provider` named (the Cloud page's Connect rows), whose setup then skips
 *  choosing the provider (`providerChosen`). An unknown provider starts as a plain new account. */
export function startingSetup(kind: string | null, provider: string | null): { form: ServerHostInput; providerChosen: boolean } {
  if (kind !== "cloud") return { form: { ...EMPTY_HOST }, providerChosen: false };
  if (provider !== null && isCloud(provider as RemoteProvider)) return { form: withProvider(EMPTY_CLOUD, provider as CloudProvider), providerChosen: true };
  return { form: { ...EMPTY_CLOUD }, providerChosen: false };
}

/** How the launcher signs in to a Proxmox node: with the player's SSH key (it then makes its own
 *  API token there), an API token, or a user and password. */
export type SignIn = "sshKey" | "token" | "password";

/** A saved host's sign-in; a new Proxmox setup starts on the SSH key. */
export function initialSignIn(v: ServerHostInput): SignIn {
  if (v.sshKey) return "sshKey";
  if (v.username.includes("!")) return "token";
  return v.id === null && v.provider === "proxmox" ? "sshKey" : "password";
}

/** The SSH key sign-in a setup starts from: the saved one, else root on port 22, its key to pick. */
export function initialSshKey(v: ServerHostInput): SshKeySignIn {
  return v.sshKey ?? { path: "", user: "root", port: 22 };
}

/** A port a field may hold: empty (the default) or a whole TCP port number. */
const portOk = (port: number | null | undefined) => port == null || (Number.isInteger(port) && port >= 1 && port <= 65535);

/** Whether the Server step (Proxmox: its name, address and API port) has what it needs. */
export function serverReady(v: ServerHostInput): boolean {
  return v.host.trim() !== "" && portOk(v.port);
}

/** Whether a Proxmox SSH key sign-in names a key, a user and a port. */
function sshKeyReady(k: SshKeySignIn): boolean {
  return k.path.trim() !== "" && k.user.trim() !== "" && portOk(k.port);
}

/** Whether two SSH key sign-ins are the same key, user and port. */
export function sameSshKey(a: SshKeySignIn | null | undefined, b: SshKeySignIn): boolean {
  return !!a && a.path.trim() === b.path.trim() && a.user.trim() === b.user.trim() && a.port === b.port;
}

/** What a save sends. An SSH key sign-in whose token isn't made yet (`made` false) goes with no
 *  username, for the node to make the token (`server_save_ssh_key`); once made, with the token
 *  id the form now holds, kept by a plain save. Password and token setups send no key. */
export function savePayload(v: ServerHostInput, signIn: SignIn, key: SshKeySignIn, made = false): ServerHostInput {
  const named = { ...v, name: saveName(v) };
  if (v.provider !== "proxmox" || signIn !== "sshKey") return { ...named, sshKey: null };
  const sshKey = { ...key, path: key.path.trim(), user: key.user.trim() };
  return made ? { ...named, password: null, sshKey } : { ...named, username: "", password: null, sshKey };
}

/** The form placed on node `nodeName` of the server's choices (the first node when it isn't one):
 *  a value the form holds stays when that node offers it, unless `reset` (the node was just
 *  changed); otherwise the node's default (`local-lvm`, `local`, the bridge on the server's LAN). */
export function placementWith(v: ServerHostInput, placement: ProxmoxPlacement, nodeName: string | null, reset = false): ServerHostInput {
  const node = placement.nodes.find((n) => n.name === nodeName) ?? placement.nodes.at(0);
  if (!node) return v;
  const keep = (value: string | null | undefined, options: string[], fallback: string | null) =>
    !reset && value && options.includes(value) ? value : fallback;
  return {
    ...v,
    node: node.name,
    datastore: keep(v.datastore, node.diskStorages, node.defaultDisk),
    imageStorage: keep(v.imageStorage, node.imageStorages, node.defaultImage),
    snippetStorage: keep(v.snippetStorage, node.snippetStorages, node.defaultSnippet),
    network: keep(
      v.network,
      node.bridges.map((b) => b.name),
      node.defaultBridge,
    ),
  };
}

/** Whether a save must first make the token on the node (an SSH key sign-in with no token yet). */
export const makesToken = (payload: ServerHostInput) => !!payload.sshKey && payload.username === "";

/** Whether the connection / credentials step has what it needs to go on. `secretKept`: a saved
 *  secret stays when the field is left empty (see `keepsSecret`). `key`: the Proxmox SSH key
 *  sign-in, when that is how it signs in. */
export function connectionReady(v: ServerHostInput, secretKept: boolean, key: SshKeySignIn | null = null): boolean {
  if (v.provider === "proxmox" && key) return serverReady(v) && sshKeyReady(key);
  const host = v.host.trim() !== "";
  const user = v.username.trim() !== "";
  const secret = secretKept || (v.password ?? "") !== "";
  if (isTokenCloud(v.provider)) return host && secret;
  if (isCliAuthCloud(v.provider) || v.provider === "oci") return host && user;
  if (isCloud(v.provider) && v.useCliCreds) return host;
  // A server's port: empty (its default) or a whole number a TCP port can be (the field took 0
  // or 70000 and went on).
  return host && user && secret && portOk(v.port);
}

/** Whether an edited host or account has a saved secret to keep: one that signed in with the
 *  cloud's CLI and now uses keys never had a secret key, so it must be typed. */
export function keepsSecret(initial: ServerHostInput, v: ServerHostInput): boolean {
  return initial.id !== null && !(initial.useCliCreds && !v.useCliCreds);
}

/** The provider's kind and how it connects, as the setup steps read it. */
export function providerTraits(provider: RemoteProvider) {
  return {
    cloud: isCloud(provider),
    aws: provider === "aws",
    azure: provider === "azure",
    gcp: provider === "gcp",
    digitalocean: provider === "digitalocean",
    linode: provider === "linode",
    oci: provider === "oci",
    tokenCloud: isTokenCloud(provider),
    cliAuth: isCliAuthCloud(provider),
    kind: KIND[provider],
  };
}

/** The clouds with a CLI of their own: the dependency that installs it, its key in the report. */
const CLIS = {
  aws: { dependency: "awscli", key: "aws" },
  azure: { dependency: "azurecli", key: "azure" },
  gcp: { dependency: "gcloud", key: "gcloud" },
} as const satisfies Record<string, { dependency: Dependency; key: keyof CloudClis }>;

/** The CLI a cloud needs on this machine (none for the token and key clouds). */
export function cloudCli(provider: CloudProvider) {
  const cli = provider === "azure" || provider === "gcp" ? CLIS[provider] : CLIS.aws;
  return { has: CLOUD_META[provider].cli !== "", ...cli };
}

/**
 * What this machine needs to drive the chosen server or cloud, and what it has: ESXi goes through
 * Vagrant, its ESXi plugin and VMware's OVF Tool; Proxmox through Terraform (installed locally);
 * a cloud through Terraform and its CLI (the token and key clouds have none).
 */
export function machineTools(provider: RemoteProvider, report: SystemReport | null) {
  const cloud = isCloud(provider);
  const cli = cloudCli(cloud ? provider : "aws");
  const status = report?.vmProviders.find((p) => p.provider === provider);
  const cloudHasCli = cloud && cli.has;
  const cloudCliTool = cloudHasCli ? report?.cloudClis[cli.key] : undefined;
  const tools = {
    vagrantOk: !!report?.vagrant.installed,
    esxiPluginOk: !!status?.pluginInstalled,
    ovftoolOk: !!report?.ovftool.installed,
    terraformOk: !!report?.terraform.installed,
    cloudHasCli,
    cloudCliTool,
    cloudCliOk: !!cloudCliTool?.installed,
    cloudDep: cli.dependency,
  };
  const toolsOk = cloud
    ? (cloudHasCli ? tools.cloudCliOk : true) && tools.terraformOk
    : provider === "vmware_esxi"
      ? tools.vagrantOk && tools.esxiPluginOk && tools.ovftoolOk
      : tools.terraformOk;
  return { ...tools, toolsOk };
}

/** The name a host is saved under: the typed one, else the cloud's short name or the address. */
export function saveName(v: ServerHostInput): string {
  const fallback = isCloud(v.provider) ? CLOUD_DEFAULT_NAME[v.provider] : v.host.trim();
  return v.name.trim() || fallback;
}

/** The setup window's title: what was saved and whether its test passed ("connected" only once
 *  the test says so: a saved host whose test failed isn't), else what is being set up. */
export function setupTitle({
  saved,
  test,
  editing,
  cloud,
  initialName,
}: {
  saved: { name: string } | null;
  test: ServerTest | "testing" | null;
  editing: boolean;
  cloud: boolean;
  initialName: string;
}) {
  const failed = !!test && test !== "testing" && !test.ok;
  if (saved) return { key: failed ? "savedNotConnected" : "connected", name: saved.name } as const;
  if (editing) return { key: "edit", name: initialName } as const;
  return { key: cloud ? "cloud" : "host", name: "" } as const;
}
