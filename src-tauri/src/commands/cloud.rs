//! The cloud accounts' commands: the CLIs' sign-ins and what each CLI can see.

use tauri::ipc::Channel;

use crate::cloud_cli::{self, AzureSubscription, Cloud, GcpBillingAccount, GcpOrganization, OciConfig};
use crate::error::Result;

use super::lines;

/// Signs in to `provider` through its CLI, the CLI's lines sent on `logs` (see [`cloud_cli::cloud_login`]).
#[tauri::command]
pub async fn cloud_login(provider: Cloud, logs: Channel<String>) -> Result<()> {
    cloud_cli::cloud_login(provider, lines(logs)).await
}

/// Cancels the CLI sign-in in progress, if any.
#[tauri::command]
pub fn cloud_login_cancel() {
    cloud_cli::cloud_login_cancel();
}

/// Signs in to AWS in the browser (`aws login`), the CLI's lines sent on `logs`.
#[tauri::command]
pub async fn aws_login(profile: Option<String>, logs: Channel<String>) -> Result<()> {
    cloud_cli::aws_login(profile, lines(logs)).await
}

/// The identity the AWS CLI resolves for `profile`, if any.
#[tauri::command]
pub async fn aws_cli_identity(profile: Option<String>) -> Option<String> {
    cloud_cli::aws_cli_identity(profile).await
}

/// This month's AWS spend so far, as the AWS CLI's `profile` sees it.
#[tauri::command]
pub async fn aws_month_to_date_cost(profile: Option<String>) -> Option<f64> {
    cloud_cli::aws_month_to_date_cost(profile).await
}

/// The AWS CLI profiles configured on this machine.
#[tauri::command]
pub async fn aws_profiles() -> Vec<String> {
    cloud_cli::aws_profiles().await
}

/// The Azure subscriptions the signed-in account can see.
#[tauri::command]
pub async fn azure_subscriptions() -> Vec<AzureSubscription> {
    cloud_cli::azure_subscriptions().await
}

/// The OCI CLI's default profile.
#[tauri::command]
pub async fn oci_config() -> OciConfig {
    cloud_cli::oci_config()
}

/// The account gcloud is signed in with, if any.
#[tauri::command]
pub async fn gcp_account() -> Option<String> {
    cloud_cli::gcp_account().await
}

/// The GCP billing accounts the signed-in user can see.
#[tauri::command]
pub async fn gcp_billing_accounts() -> Vec<GcpBillingAccount> {
    cloud_cli::gcp_billing_accounts().await
}

/// The GCP organizations the signed-in user belongs to.
#[tauri::command]
pub async fn gcp_organizations() -> Vec<GcpOrganization> {
    cloud_cli::gcp_organizations().await
}
