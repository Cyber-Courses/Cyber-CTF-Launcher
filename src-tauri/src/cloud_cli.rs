//! What the launcher reads and runs through the clouds' own CLIs: their browser sign-ins (`az
//! login`, `gcloud auth`, `aws login`) and cancelling one, the AWS identity, profiles and the
//! month's cost so far, Azure's subscriptions, GCP's account, billing accounts and organizations,
//! and OCI's `~/.oci/config`. An account signed in this way keeps no secret in the launcher:
//! Terraform uses the CLI's credential chain. (AWS access keys and the token clouds' tokens are
//! the keychain's, `runtime::server::secrets`.)

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::error::{Code, Error, Result};
use crate::platform::exec::{run_cli, stream};
use crate::runtime::server::sign_in::{AWS_WHO_AM_I, sign_in_question};

/// The clouds `cloud_login` signs in to through their CLI. AWS signs in with [`aws_login`]
/// instead: its browser sign-in takes the profile to sign in.
#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "CloudLoginProvider")]
pub enum Cloud {
    /// Azure, through `az login`.
    Azure,
    /// Google Cloud, through `gcloud auth login --update-adc`.
    Gcp,
}

/// Signs in to Azure or GCP with its CLI, in the browser. Streams the CLI output to the UI,
/// until it ends, the player cancels it (`cloud_login_cancel`) or [`LOGIN_TIMEOUT`] passes.
pub async fn cloud_login(provider: Cloud, on_line: impl FnMut(String)) -> Result<()> {
    let (program, args) = login_command(&provider);
    sign_in(program, args, on_line).await
}

/// How long a CLI sign-in waits for the player to finish it in the browser.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The CLI sign-in in progress: sending on it cancels it.
static SIGN_IN: Mutex<Option<oneshot::Sender<()>>> = Mutex::new(None);

/// Cancels the CLI sign-in in progress (its CLI is stopped), if any.
pub fn cloud_login_cancel() {
    if let Some(cancel) = SIGN_IN.lock().ok().and_then(|mut s| s.take()) {
        let _ = cancel.send(());
    }
}

/// Runs a CLI sign-in (`program args`), its lines to `on_line`, until it ends, is cancelled, or
/// [`LOGIN_TIMEOUT`] passes. A new sign-in cancels the one before it.
async fn sign_in(program: &'static str, args: &[&str], mut on_line: impl FnMut(String)) -> Result<()> {
    on_line(format!("$ {program} {}", args.join(" ")));
    sign_in_within(LOGIN_TIMEOUT, stream(program, args, None, &[], on_line), cancellable()).await
}

/// A cancel for a new sign-in, through `cloud_login_cancel`; the one before it is cancelled.
fn cancellable() -> oneshot::Receiver<()> {
    let (tx, cancelled) = oneshot::channel();
    if let Some(previous) = SIGN_IN.lock().ok().and_then(|mut s| s.replace(tx)) {
        let _ = previous.send(());
    }
    cancelled
}

/// `signing_in` until it ends, `cancelled` fires, or `limit` passes. Ended either way, it is
/// dropped: its CLI is killed with what it started.
async fn sign_in_within(limit: Duration, signing_in: impl Future<Output = Result<()>>, cancelled: oneshot::Receiver<()>) -> Result<()> {
    tokio::select! {
        // A sign-in that is done wins over a cancel that came at the same time.
        biased;
        done = tokio::time::timeout(limit, signing_in) => done.unwrap_or_else(|_| Err(Error::coded(Code::LoginTimeout { minutes: limit.as_secs() / 60 }))),
        _ = cancelled => Err(Error::Cancelled),
    }
}

/// The CLI sign-in for a cloud.
fn login_command(provider: &Cloud) -> (&'static str, &'static [&'static str]) {
    match provider {
        Cloud::Azure => ("az", &["login"]),
        // One browser flow that authenticates the gcloud CLI (so `gcloud projects list` works)
        // AND writes Application Default Credentials (what Terraform's google provider reads).
        // Plain `auth application-default login` only does the latter, leaving the CLI unauthed.
        Cloud::Gcp => ("gcloud", &["auth", "login", "--update-adc"]),
    }
}

/// `args`, then `--profile <p>` when a profile is given.
fn with_profile<'a>(mut args: Vec<&'a str>, profile: Option<&'a str>) -> Vec<&'a str> {
    if let Some(p) = profile {
        args.push("--profile");
        args.push(p);
    }
    args
}

/// A CLI's one-line answer, trimmed; None when blank.
fn non_empty(out: &str) -> Option<String> {
    Some(out.trim().to_string()).filter(|s| !s.is_empty())
}

/// The identity the host AWS CLI resolves (for the given profile, or the default chain),
/// if any, so the cloud setup can show "signed in as …" and offer using the CLI instead of
/// pasting keys. None when the CLI is missing or that profile has no usable credentials (logged,
/// but for a CLI that isn't installed).
pub async fn aws_cli_identity(profile: Option<String>) -> Option<String> {
    // The question a launch asks (see `sign_in_question`), which answers with the identity.
    let args = with_profile(AWS_WHO_AM_I.to_vec(), profile.as_deref());
    answered("the AWS CLI's identity", run_cli("aws", &args).await).as_deref().and_then(non_empty)
}

/// This month's AWS spend so far in USD, as the AWS CLI's `profile` (or its default) sees it:
/// the setup form's, before the account is saved (a saved account's is `server_month_to_date_cost`,
/// with its own credentials). None when the CLI or Cost Explorer isn't available (logged, but for
/// a CLI that isn't installed).
pub async fn aws_month_to_date_cost(profile: Option<String>) -> Option<f64> {
    answered("this month's AWS spend", crate::runtime::server::cli_month_to_date_cost(profile).await)
}

/// The AWS CLI profiles configured on this machine (`aws configure list-profiles`), so the
/// user can pick one when they have several. Empty when the CLI is missing or has none, or fails
/// (logged).
pub async fn aws_profiles() -> Vec<String> {
    answered("the AWS CLI profiles", run_cli("aws", &["configure", "list-profiles"]).await).map(|s| lines(&s)).unwrap_or_default()
}

/// What a CLI answered when asked for `what`; none when it couldn't answer. A CLI that isn't
/// installed is the one expected case; any other failure (signed out, an API turned off, a
/// permission denied, a timeout) is logged, so a setup showing nothing can be explained.
fn answered<T>(what: &str, answer: Result<T>) -> Option<T> {
    match answer {
        Ok(found) => Some(found),
        Err(Error::ToolMissing { .. }) => None,
        Err(e) => {
            log::warn!("couldn't read {what}, so none is offered: {e}");
            None
        }
    }
}

/// A CLI's JSON list of `what`; empty when it doesn't parse (logged: a changed
/// output format).
fn parsed<T: serde::de::DeserializeOwned>(what: &str, out: &str) -> Vec<T> {
    serde_json::from_str(out).unwrap_or_else(|e| {
        log::warn!("couldn't read {what} from the CLI's answer, so none is offered: {e}");
        Vec::new()
    })
}

/// The non-blank lines of a CLI's output, trimmed.
fn lines(out: &str) -> Vec<String> {
    out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// An Azure subscription the signed-in account can use.
#[derive(Serialize, Deserialize, Default, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AzureSubscription {
    /// The subscription's display name.
    pub name: String,
    /// The subscription's id.
    pub id: String,
    /// Whether the Azure CLI has it as its default subscription.
    #[serde(default)]
    pub is_default: bool,
}

/// The Azure subscriptions the signed-in account can see (`az account list`), so the user can
/// pick one instead of pasting a GUID. Empty when the CLI is missing or not signed in, or fails
/// (logged).
pub async fn azure_subscriptions() -> Vec<AzureSubscription> {
    let out = run_cli("az", &["account", "list", "--query", "[].{name:name,id:id,isDefault:isDefault}", "--output", "json"]).await;
    answered(SUBSCRIPTIONS, out).map(|out| subscriptions(&out)).unwrap_or_default()
}

/// What the Azure subscriptions are called in the log.
const SUBSCRIPTIONS: &str = "the Azure subscriptions";

/// `az account list` output (the query above) as subscriptions; none when it doesn't parse.
fn subscriptions(out: &str) -> Vec<AzureSubscription> {
    parsed(SUBSCRIPTIONS, out)
}

/// A GCP billing account the signed-in user can see.
#[derive(Serialize, Default, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GcpBillingAccount {
    /// The billing account's id (`billingAccounts/` left out).
    pub id: String,
    /// Its display name.
    pub name: String,
    /// Whether it is open, so it can pay for a project.
    pub open: bool,
}

/// A GCP organization the signed-in user belongs to.
#[derive(Serialize, Default, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GcpOrganization {
    /// The organization's numeric id.
    pub id: String,
    /// Its display name.
    pub name: String,
}

/// What the launcher found in ~/.oci/config (DEFAULT profile), to prefill the OCI setup.
#[derive(Serialize, Default, Debug, PartialEq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct OciConfig {
    /// Whether the config file was found and read.
    pub configured: bool,
    /// The DEFAULT profile's tenancy OCID, empty when it has none.
    pub tenancy: String,
    /// The DEFAULT profile's region, empty when it has none.
    pub region: String,
}

/// Reads the OCI config (`OCI_CLI_CONFIG_FILE`, else ~/.oci/config) DEFAULT profile, so the setup
/// can show whether it's configured and prefill the tenancy (as the compartment) and region.
pub fn oci_config() -> OciConfig {
    read_oci_config(oci_config_path().as_deref())
}

/// `OCI_CLI_CONFIG_FILE`, else ~/.oci/config.
fn oci_config_path() -> Option<std::path::PathBuf> {
    std::env::var_os("OCI_CLI_CONFIG_FILE").map(std::path::PathBuf::from).or_else(|| crate::platform::home_dir().map(|h| h.join(".oci").join("config")))
}

/// The config file at `path`; not configured when there is none or it can't be read.
fn read_oci_config(path: Option<&std::path::Path>) -> OciConfig {
    let Some(path) = path else { return OciConfig::default() };
    let Ok(text) = std::fs::read_to_string(path) else { return OciConfig::default() };
    parse_oci_config(&text)
}

/// The DEFAULT profile's tenancy and region in an OCI config file's text.
fn parse_oci_config(text: &str) -> OciConfig {
    let (mut tenancy, mut region) = (String::new(), String::new());
    let mut in_default = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_default = l.eq_ignore_ascii_case("[DEFAULT]");
            continue;
        }
        if !in_default {
            continue;
        }
        if let Some((k, val)) = l.split_once('=') {
            match k.trim() {
                "tenancy" => tenancy = val.trim().to_string(),
                "region" => region = val.trim().to_string(),
                _ => {}
            }
        }
    }
    OciConfig { configured: true, tenancy, region }
}

/// The active gcloud account email, or None if the CLI isn't signed in. Reliable even when
/// `projects list` is empty or the Resource Manager API is off, so the UI can show "signed in".
/// An account whose sign-in expired (Workspace reauthentication) still shows as active in
/// `auth list` while every call fails: it counts as signed out, so the UI offers to sign in
/// again instead of showing an empty billing list. So does one whose application-default
/// sign-in, the one Terraform uses, ran out on its own: a launch asks for that one first (see
/// `sign_in_question`), and would be refused.
pub async fn gcp_account() -> Option<String> {
    let accounts =
        answered("the active Google Cloud account", run_cli("gcloud", &["auth", "list", "--filter=status:ACTIVE", "--format=value(account)"]).await)?;
    let email = non_empty(&accounts)?;
    answered("a Google Cloud access token", run_cli("gcloud", &["auth", "print-access-token", "--quiet"]).await)?;
    let (program, question) = sign_in_question(crate::runtime::providers::Provider::Gcp)?;
    answered("Google Cloud's application-default sign-in", run_cli(program, question).await)?;
    Some(email)
}

/// The GCP billing accounts the signed-in user can see (`gcloud billing accounts list`), so they
/// pick one for the per-lab projects. Empty when the CLI is missing or not signed in, or fails
/// (logged).
pub async fn gcp_billing_accounts() -> Vec<GcpBillingAccount> {
    answered(BILLING_ACCOUNTS, run_cli("gcloud", &["billing", "accounts", "list", "--format", "json"]).await)
        .map(|out| billing_accounts(&out))
        .unwrap_or_default()
}

/// What the GCP billing accounts are called in the log.
const BILLING_ACCOUNTS: &str = "the Google Cloud billing accounts";

/// `gcloud billing accounts list --format json` as billing accounts (ids without their prefix);
/// none when it doesn't parse.
fn billing_accounts(out: &str) -> Vec<GcpBillingAccount> {
    parsed::<serde_json::Value>(BILLING_ACCOUNTS, out)
        .into_iter()
        .map(|v| GcpBillingAccount {
            id: v["name"].as_str().unwrap_or_default().trim_start_matches("billingAccounts/").to_string(),
            name: v["displayName"].as_str().unwrap_or_default().to_string(),
            open: v["open"].as_bool().unwrap_or(false),
        })
        .filter(|b| !b.id.is_empty())
        .collect()
}

/// The GCP organizations the signed-in user belongs to (`gcloud organizations list`). Empty for
/// a personal / no-org account, so the user can create projects without a parent; and when the CLI
/// fails (logged).
pub async fn gcp_organizations() -> Vec<GcpOrganization> {
    answered(ORGANIZATIONS, run_cli("gcloud", &["organizations", "list", "--format", "json"]).await).map(|out| organizations(&out)).unwrap_or_default()
}

/// What the GCP organizations are called in the log.
const ORGANIZATIONS: &str = "the Google Cloud organizations";

/// `gcloud organizations list --format json` as organizations (ids without their prefix); none
/// when it doesn't parse.
fn organizations(out: &str) -> Vec<GcpOrganization> {
    parsed::<serde_json::Value>(ORGANIZATIONS, out)
        .into_iter()
        .map(|v| GcpOrganization {
            id: v["name"].as_str().unwrap_or_default().trim_start_matches("organizations/").to_string(),
            name: v["displayName"].as_str().unwrap_or_default().to_string(),
        })
        .filter(|o| !o.id.is_empty())
        .collect()
}

/// Signs in to AWS in the browser (`aws login`, AWS CLI >= 2.32.0): console credentials for
/// root / IAM / federation, temporary credentials for up to 12 hours, no SSO setup needed.
/// Streams the CLI output.
pub async fn aws_login(profile: Option<String>, on_line: impl FnMut(String)) -> Result<()> {
    let args = with_profile(vec!["login"], profile.as_deref());
    sign_in("aws", &args, on_line).await
}

#[cfg(test)]
mod tests;
