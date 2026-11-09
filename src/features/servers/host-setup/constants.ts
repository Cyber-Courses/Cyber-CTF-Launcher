import { PROVIDER_NAMES, type CloudProvider, type ServerHostInput, type RemoteProvider } from "@/lib/tauri";

// The server setup flow, a step-by-step wizard shown in its own window (src/app/server-setup).
// Matches the "Set up this machine" wizard (features/machine/machine-setup.tsx).

export const KIND: Record<RemoteProvider, { label: string; note: string; port: number; user: string; plugin: string }> = {
  proxmox: { label: "Proxmox VE", note: "Signs in to the Proxmox API", port: 8006, user: "root", plugin: "vagrant-proxmox" },
  vmware_esxi: { label: "VMware ESXi", note: "Drives the host over SSH", port: 22, user: "root", plugin: "vagrant-vmware-esxi" },
  aws: { label: PROVIDER_NAMES.aws, note: "EC2 in your own account", port: 443, user: "AKIA…", plugin: "" },
  azure: { label: PROVIDER_NAMES.azure, note: "VMs in your own subscription", port: 443, user: "subscription id", plugin: "" },
  gcp: { label: PROVIDER_NAMES.gcp, note: "VMs in your own project", port: 443, user: "project id", plugin: "" },
  digitalocean: { label: PROVIDER_NAMES.digitalocean, note: "Droplets in your own account", port: 443, user: "API token", plugin: "" },
  linode: { label: PROVIDER_NAMES.linode, note: "Linodes in your own account", port: 443, user: "API token", plugin: "" },
  oci: { label: PROVIDER_NAMES.oci, note: "Instances in your own tenancy", port: 443, user: "compartment OCID", plugin: "" },
};

/** Server hypervisors, as opposed to cloud accounts. */
export const SERVER_KINDS: RemoteProvider[] = ["proxmox", "vmware_esxi"];

export const EMPTY_HOST: ServerHostInput = {
  id: null,
  name: "",
  provider: "proxmox",
  host: "",
  port: null,
  username: "",
  password: null,
  datastore: null,
  network: null,
  node: null,
  // Proxmox ships with a self-signed certificate.
  insecureTls: true,
  autoStopHours: null,
};

/** A new AWS account: region + access keys. */
export const EMPTY_CLOUD: ServerHostInput = { ...EMPTY_HOST, provider: "aws", host: "eu-west-3", useCliCreds: true, insecureTls: false, autoStopHours: 4 };

/** Common AWS regions for the cloud setup dropdown (code, human name). */
export const AWS_REGIONS: [string, string][] = [
  ["us-east-1", "US East (N. Virginia)"],
  ["us-east-2", "US East (Ohio)"],
  ["us-west-2", "US West (Oregon)"],
  ["eu-west-1", "Europe (Ireland)"],
  ["eu-west-3", "Europe (Paris)"],
  ["eu-central-1", "Europe (Frankfurt)"],
  ["ap-southeast-1", "Asia Pacific (Singapore)"],
  ["ap-northeast-1", "Asia Pacific (Tokyo)"],
];

/** Common Azure locations for the cloud setup dropdown (id, human name). */
export const AZURE_LOCATIONS: [string, string][] = [
  // First: it accepts new subscriptions (busy regions such as West Europe may not).
  ["swedencentral", "Sweden Central (Gävle)"],
  ["italynorth", "Italy North (Milan)"],
  ["westeurope", "West Europe (Netherlands)"],
  ["northeurope", "North Europe (Ireland)"],
  ["francecentral", "France Central (Paris)"],
  ["uksouth", "UK South (London)"],
  ["germanywestcentral", "Germany West Central"],
  ["eastus", "East US (Virginia)"],
  ["westus2", "West US 2 (Washington)"],
  ["southeastasia", "Southeast Asia (Singapore)"],
];

/** Common GCP regions for the cloud setup dropdown (id, human name). The lab host lands in the region's -b zone. */
export const GCP_REGIONS: [string, string][] = [
  ["europe-west1", "Belgium"],
  ["europe-west2", "London"],
  ["europe-west3", "Frankfurt"],
  ["europe-west9", "Paris"],
  ["us-central1", "Iowa"],
  ["us-east1", "South Carolina"],
  ["us-west1", "Oregon"],
  ["asia-southeast1", "Singapore"],
];

/** Common DigitalOcean regions for the cloud setup dropdown (slug, human name). */
export const DO_REGIONS: [string, string][] = [
  ["fra1", "Frankfurt"],
  ["ams3", "Amsterdam"],
  ["lon1", "London"],
  ["nyc3", "New York"],
  ["sfo3", "San Francisco"],
  ["tor1", "Toronto"],
  ["sgp1", "Singapore"],
  ["blr1", "Bangalore"],
  ["syd1", "Sydney"],
];

/** Common Linode regions for the cloud setup dropdown (slug, human name). */
export const LINODE_REGIONS: [string, string][] = [
  ["eu-central", "Frankfurt"],
  ["eu-west", "London"],
  ["us-east", "Newark"],
  ["us-central", "Dallas"],
  ["us-west", "Fremont"],
  ["ap-south", "Singapore"],
  ["ap-northeast", "Tokyo"],
  ["ca-central", "Toronto"],
];

/** Common OCI regions for the cloud setup dropdown (id, human name). */
export const OCI_REGIONS: [string, string][] = [
  ["eu-frankfurt-1", "Frankfurt"],
  ["eu-amsterdam-1", "Amsterdam"],
  ["uk-london-1", "London"],
  ["us-ashburn-1", "Ashburn"],
  ["us-phoenix-1", "Phoenix"],
  ["ap-singapore-1", "Singapore"],
  ["ap-tokyo-1", "Tokyo"],
  ["ca-toronto-1", "Toronto"],
];

export type StepKey = "provider" | "hypervisor" | "tools" | "server" | "connection" | "placement" | "account" | "credentials" | "options" | "test";
/** Each cloud, in the order the launcher lists them: the one table of the clouds the rest
 *  derive from. Its brand's full name (`label`: the picker, the first-run tiles, the logo's alt
 *  text; its short name, in lists and places and as a new account's default name, is the
 *  backend's `PROVIDER_NAMES`), its CLI (none for the token and key clouds), and whether it can
 *  be picked (`ready`). Each has its logo at `/brands/<id>.svg`. */
export const CLOUD_META: Record<CloudProvider, { label: string; cli: string; ready: boolean }> = {
  aws: { label: "Amazon Web Services", cli: "aws", ready: true },
  azure: { label: "Microsoft Azure", cli: "az", ready: true },
  gcp: { label: "Google Cloud", cli: "gcloud", ready: true },
  digitalocean: { label: "DigitalOcean", cli: "", ready: true },
  linode: { label: "Linode", cli: "", ready: true },
  oci: { label: "Oracle Cloud", cli: "", ready: true },
};

/** Every cloud, in the order the launcher lists them. */
export const CLOUD_IDS = Object.keys(CLOUD_META) as CloudProvider[];

/** A cloud's logo, in public/brands. */
export const cloudLogo = (id: string) => `/brands/${id}.svg`;
