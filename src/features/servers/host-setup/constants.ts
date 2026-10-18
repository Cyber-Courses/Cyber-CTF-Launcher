import { type CloudProvider, type ServerHostInput, type RemoteProvider } from "@/lib/tauri";

// The server setup flow, a step-by-step wizard shown in its own window (src/app/server-setup).
// Matches the "Set up this machine" wizard (features/machine/machine-setup.tsx).

export const KIND: Record<RemoteProvider, { label: string; note: string; port: number; user: string; plugin: string }> = {
  proxmox: { label: "Proxmox VE", note: "Signs in to the Proxmox API", port: 8006, user: "root@pam", plugin: "vagrant-proxmox" },
  vmware_esxi: { label: "VMware ESXi", note: "Drives the host over SSH", port: 22, user: "root", plugin: "vagrant-vmware-esxi" },
  aws: { label: "AWS", note: "EC2 in your own account", port: 443, user: "AKIA…", plugin: "" },
  azure: { label: "Azure", note: "VMs in your own subscription", port: 443, user: "subscription id", plugin: "" },
  gcp: { label: "Google Cloud", note: "VMs in your own project", port: 443, user: "project id", plugin: "" },
  digitalocean: { label: "DigitalOcean", note: "Droplets in your own account", port: 443, user: "API token", plugin: "" },
  linode: { label: "Linode", note: "Linodes in your own account", port: 443, user: "API token", plugin: "" },
  oci: { label: "Oracle Cloud", note: "Instances in your own tenancy", port: 443, user: "compartment OCID", plugin: "" },
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

export type StepKey = "provider" | "hypervisor" | "tools" | "connection" | "placement" | "account" | "credentials" | "options" | "test";
/** A cloud host's default name: the provider's short name (the user can rename it). */
export const CLOUD_DEFAULT_NAME: Record<CloudProvider, string> = {
  aws: "AWS",
  azure: "Azure",
  gcp: "GCP",
  digitalocean: "DigitalOcean",
  linode: "Linode",
  oci: "Oracle Cloud",
};

/** Each cloud's full name and CLI (none for the token and key clouds); `ready` gates which can
 *  be picked. */
export const CLOUD_META: Record<CloudProvider, { label: string; cli: string; ready: boolean }> = {
  aws: { label: "Amazon Web Services", cli: "aws", ready: true },
  azure: { label: "Microsoft Azure", cli: "az", ready: true },
  gcp: { label: "Google Cloud", cli: "gcloud", ready: true },
  digitalocean: { label: "DigitalOcean", cli: "", ready: true },
  linode: { label: "Linode", cli: "", ready: true },
  oci: { label: "Oracle Cloud", cli: "", ready: true },
};

/** The provider picker. `logo` marks the ones with an SVG in public/brands (others fall back to
 *  a cloud icon). */
export const CLOUD_PICKER: { id: CloudProvider; label: string; ready: boolean; logo: boolean }[] = [
  { id: "aws", label: "Amazon Web Services", ready: true, logo: true },
  { id: "azure", label: "Microsoft Azure", ready: true, logo: true },
  { id: "gcp", label: "Google Cloud", ready: true, logo: true },
  { id: "digitalocean", label: "DigitalOcean", ready: true, logo: true },
  { id: "linode", label: "Linode", ready: true, logo: true },
  { id: "oci", label: "Oracle Cloud", ready: true, logo: true },
];
