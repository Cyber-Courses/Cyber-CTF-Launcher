import { DEFAULT_AUTO_STOP_HOURS, type CloudProvider, type ServerHostInput, type ServerProvider } from "@/lib/tauri";

// The server setup flow, a step-by-step wizard shown in its own window (src/app/server-setup).
// Matches the "Set up this machine" wizard (features/machine/machine-setup.tsx).

/** The user each server signs in with by default, the connection form's placeholder (its port is
 *  the backend's `HOST_KINDS`). */
export const SERVER_USER: Record<ServerProvider, string> = { proxmox: "root", vmware_esxi: "root" };

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

/** Where a new account's labs run by default, per cloud. */
export const DEFAULT_REGION: Record<CloudProvider, string> = {
  aws: "eu-west-3",
  azure: "swedencentral",
  gcp: "europe-west1",
  digitalocean: "fra1",
  linode: "eu-central",
  oci: "eu-frankfurt-1",
};

/** A new AWS account: its default region, signed in with the AWS CLI, the default auto-stop. */
export const EMPTY_CLOUD: ServerHostInput = {
  ...EMPTY_HOST,
  provider: "aws",
  host: DEFAULT_REGION.aws,
  useCliCreds: true,
  insecureTls: false,
  autoStopHours: DEFAULT_AUTO_STOP_HOURS,
};

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
/** A cloud's logo, in public/brands. */
export const cloudLogo = (id: string) => `/brands/${id}.svg`;
