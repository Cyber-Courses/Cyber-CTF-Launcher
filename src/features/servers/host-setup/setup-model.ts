import type { CloudProvider, Dependency, RemoteProvider, ServerHostInput, ServerTest, SystemReport } from "@/lib/tauri";
import {
  AWS_REGIONS,
  AZURE_LOCATIONS,
  CLOUD_DEFAULT_NAME,
  CLOUD_META,
  DO_REGIONS,
  GCP_REGIONS,
  LINODE_REGIONS,
  OCI_REGIONS,
  type StepKey,
} from "@/features/servers/host-setup/constants";

// The server / cloud setup's logic, as pure functions of the form and what this machine has.

const CLOUDS: readonly string[] = Object.keys(CLOUD_DEFAULT_NAME);
const DEFAULT_NAMES = Object.values(CLOUD_DEFAULT_NAME);

export const isCloud = (p: RemoteProvider): p is CloudProvider => CLOUDS.includes(p);

/** Token clouds (DigitalOcean, Linode) authenticate with just a pasted API token (no CLI, no username). */
export const isTokenCloud = (p: RemoteProvider) => p === "digitalocean" || p === "linode";

/** Azure and GCP authenticate through their CLI (no access keys); the flow is the same shape. */
export const isCliAuthCloud = (p: RemoteProvider) => p === "azure" || p === "gcp";

/**
 * The ordered steps for this setup. Editing skips choosing the provider or hypervisor and
 * installing the tools; only AWS asks how to connect (CLI or access keys).
 */
export function setupSteps(provider: RemoteProvider, editing: boolean): StepKey[] {
  if (!isCloud(provider)) return editing ? ["connection", "placement", "test"] : ["hypervisor", "tools", "connection", "placement", "test"];
  const account: StepKey[] = provider === "aws" ? ["account"] : [];
  return [...(editing ? [] : (["provider", "tools"] as StepKey[])), ...account, "credentials", "options", "test"];
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

/** Whether the connection / credentials step has what it needs to go on. */
export function connectionReady(v: ServerHostInput, editing: boolean): boolean {
  const host = v.host.trim() !== "";
  const user = v.username.trim() !== "";
  const secret = editing || (v.password ?? "") !== "";
  if (isTokenCloud(v.provider)) return host && secret;
  if (isCliAuthCloud(v.provider) || v.provider === "oci") return host && user;
  if (isCloud(v.provider) && v.useCliCreds) return host;
  return host && user && secret;
}

/** The CLI a cloud needs on this machine (none for the token and key clouds). */
export function cloudCli(provider: CloudProvider) {
  return {
    has: CLOUD_META[provider].cli !== "",
    dependency: (provider === "azure" ? "azurecli" : provider === "gcp" ? "gcloud" : "awscli") as Dependency,
    key: (provider === "gcp" ? "gcloud" : provider === "azure" ? "azure" : "aws") as "aws" | "azure" | "gcloud",
  };
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
    ovftoolOk: !!report?.ovftool?.installed,
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
  const fallback = isCloud(v.provider) ? (CLOUD_DEFAULT_NAME[v.provider] ?? v.host.trim()) : v.host.trim();
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
