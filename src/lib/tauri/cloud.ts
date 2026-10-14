import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming } from "@/lib/tauri/stream";
import type { AzureSubscription, CloudLoginProvider, GcpBillingAccount, GcpOrganization, OciConfig, Provider } from "@/lib/tauri/generated";

export type { AzureSubscription, CloudLoginProvider, GcpBillingAccount, GcpOrganization, OciConfig };

/** The cloud accounts a lab can run in (the cloud ones of the providers). */
export type CloudProvider = Extract<Provider, "aws" | "azure" | "gcp" | "digitalocean" | "linode" | "oci">;

/** Signs in to a cloud provider using its CLI's own auth (browser flow): AWS, Azure or GCP. */
export const cloudLogin = (provider: CloudLoginProvider, onLog: (line: string) => void) => invokeStreaming("cloud_login", { provider }, onLog);

/** Cancels the CLI sign-in in progress (`cloudLogin` or `awsLogin`), which then rejects. */
export const cloudLoginCancel = () => invoke<void>("cloud_login_cancel");

// --- AWS ---

/** The identity the AWS CLI resolves for a profile (or the default chain), or null. */
export const awsCliIdentity = (profile?: string) => invoke<string | null>("aws_cli_identity", { profile: profile ?? null });

/** AWS CLI profiles configured on this machine. */
export const awsProfiles = () => invoke<string[]>("aws_profiles");

/** This month's AWS spend so far (USD) as the AWS CLI's profile sees it (the setup form, before
 *  the account is saved), or null if unavailable. A saved account's is `serverMonthToDateCost`. */
export const awsMonthToDateCost = (profile?: string) => invoke<number | null>("aws_month_to_date_cost", { profile: profile ?? null });

/** Browser sign-in for an AWS profile (or the default) via `aws login`, streaming output. */
export const awsLogin = (profile: string | null, onLog: (line: string) => void) => invokeStreaming("aws_login", { profile }, onLog);

// --- Azure ---

/** Subscriptions the signed-in Azure account can see (`az account list`). Empty if not signed in. */
export const azureSubscriptions = () => invoke<AzureSubscription[]>("azure_subscriptions");

// --- Google Cloud ---

/** The active gcloud account email, or null if the CLI isn't signed in. */
export const gcpAccount = () => invoke<string | null>("gcp_account");

/** Billing accounts the signed-in GCP user can see (for the per-lab projects). */
export const gcpBillingAccounts = () => invoke<GcpBillingAccount[]>("gcp_billing_accounts");

/** Organizations the signed-in GCP user belongs to (empty = personal / no-org account). */
export const gcpOrganizations = () => invoke<GcpOrganization[]>("gcp_organizations");

// --- Oracle Cloud ---

/** Reads ~/.oci/config (DEFAULT profile) for the OCI setup prefill. */
export const ociConfig = () => invoke<OciConfig>("oci_config");
