//! What the cloud setup reads of the accounts the clouds' own CLIs are signed in to, and their
//! browser sign-ins (`az login`, `gcloud auth`, `aws login`) and cancelling one: the AWS identity,
//! profiles and the month's cost so far, Azure's subscriptions, GCP's account, billing accounts and
//! organizations, and OCI's `~/.oci/config`. isothrum-machine asks the CLIs
//! ([`isothrum_machine::cloud`]); the launcher keeps what the webview is sent, the log's words and
//! the sign-in's time limit and cancel. An account signed in this way keeps no secret in the
//! launcher: Terraform uses the CLI's credential chain. (AWS access keys and the token clouds'
//! tokens are the keychain's, `runtime::server::secrets`.)

use std::sync::Mutex;
use std::time::Duration;

use isothrum_machine::Failure;
use isothrum_machine::cloud::{self, aws, azure, gcp, oci};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::error::{Code, Error, Result};

/// What the cloud CLIs are asked with: isothrum-machine's context for this machine (a test's
/// scripted tools in a test).
pub(crate) fn machine() -> isothrum_machine::Context {
    crate::runtime::providers::machine_cloud()
}

/// A cloud CLI's failure as the launcher's error: a CLI that isn't installed, one that ran out of
/// time (as a timed tool's), else what it said.
pub(crate) fn tool_error(failure: &Failure) -> Error {
    let program = failure.program();
    match failure {
        Failure::Missing { .. } => match [aws::CLI, azure::CLI, gcp::CLI, oci::CLI, "docker", "terraform"].into_iter().find(|p| *p == program) {
            Some(tool) => Error::ToolMissing { tool },
            None => Error::CommandFailed { command: program.to_owned(), stderr: failure.to_string() },
        },
        Failure::TimedOut { limit, .. } => {
            Error::CommandFailed { command: program.to_owned(), stderr: format!("timed out after {}s (the tool or its backend may be stuck)", limit.as_secs()) }
        }
        _ => Error::CommandFailed { command: program.to_owned(), stderr: failure.to_string() },
    }
}

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
    let which = match provider {
        Cloud::Azure => cloud::SignIn::Azure,
        Cloud::Gcp => cloud::SignIn::Gcp,
    };
    sign_in(&which, on_line).await
}

/// How long a CLI sign-in waits for the player to finish it in the browser.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The CLI sign-in in progress: sending on it cancels it.
static SIGN_IN: Mutex<Option<oneshot::Sender<()>>> = Mutex::new(None);

/// Cancels the CLI sign-in in progress (its CLI is stopped), if any.
pub fn cloud_login_cancel() {
    if let Some(cancel) = SIGN_IN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
        let _ = cancel.send(());
    }
}

/// Runs a CLI sign-in, its lines to `on_line` as they come, until it ends, is cancelled, or
/// [`LOGIN_TIMEOUT`] passes. A new sign-in cancels the one before it.
async fn sign_in(which: &cloud::SignIn, mut on_line: impl FnMut(String)) -> Result<()> {
    let cx = machine();
    // The CLI's lines come on the runner's side; `on_line` (the command line's output, say) stays
    // on this one.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let run = async move {
        let mut each = |line: &str| tx.send(line.to_owned()).ok().unwrap_or_default();
        cloud::sign_in(&cx, which, &mut each).await
    };
    let signing_in = async {
        let relay = async {
            while let Some(line) = rx.recv().await {
                on_line(line);
            }
        };
        let (done, ()) = tokio::join!(run, relay);
        done.map_err(|f| tool_error(&f))
    };
    sign_in_within(LOGIN_TIMEOUT, signing_in, cancellable()).await
}

/// A cancel for a new sign-in, through `cloud_login_cancel`; the one before it is cancelled.
fn cancellable() -> oneshot::Receiver<()> {
    let (tx, cancelled) = oneshot::channel();
    if let Some(previous) = SIGN_IN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).replace(tx) {
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

/// An AWS account signed in through the CLI's `profile` (or its default), its calls to `region`.
fn cli_account(profile: Option<String>, region: Option<&str>) -> aws::Account {
    aws::Account::new(aws::Credentials::Cli { profile }, region.map(str::to_owned))
}

/// The identity the host AWS CLI resolves (for the given profile, or the default chain),
/// if any, so the cloud setup can show "signed in as …" and offer using the CLI instead of
/// pasting keys. None when the CLI is missing or that profile has no usable credentials (logged,
/// but for a CLI that isn't installed).
pub async fn aws_cli_identity(profile: Option<String>) -> Option<String> {
    // The question a launch asks, which answers with the identity.
    answered("the AWS CLI's identity", aws::identity(&machine(), &cli_account(profile, None)).await).filter(|arn| !arn.is_empty())
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
    answered("the AWS CLI profiles", aws::profiles(&machine()).await).unwrap_or_default()
}

/// What a CLI answered when asked for `what`; none when it couldn't answer. A CLI that isn't
/// installed is the one expected case; any other failure (signed out, an API turned off, a
/// permission denied, a timeout, an answer that doesn't read) is logged, so a setup showing
/// nothing can be explained.
fn answered<T, E: Into<Said>>(what: &str, answer: std::result::Result<T, E>) -> Option<T> {
    match answer.map_err(Into::into) {
        Ok(found) => Some(found),
        Err(Said::Missing) => None,
        Err(Said::Unreadable(e)) => {
            log::warn!("couldn't read {what} from the CLI's answer, so none is offered: {e}");
            None
        }
        Err(Said::Failed(e)) => {
            log::warn!("couldn't read {what}, so none is offered: {e}");
            None
        }
    }
}

/// Why a CLI gave no answer, as [`answered`] logs it.
enum Said {
    /// The CLI isn't installed.
    Missing,
    /// Its answer doesn't read as the list asked for (a changed output format).
    Unreadable(String),
    /// Any other failure.
    Failed(String),
}

impl From<Failure> for Said {
    fn from(f: Failure) -> Said {
        match f {
            Failure::Missing { .. } => Said::Missing,
            Failure::Unreadable { .. } => Said::Unreadable(f.to_string()),
            other => Said::Failed(tool_error(&other).to_string()),
        }
    }
}

impl From<Error> for Said {
    fn from(e: Error) -> Said {
        match e {
            Error::ToolMissing { .. } => Said::Missing,
            other => Said::Failed(other.to_string()),
        }
    }
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
    answered("the Azure subscriptions", azure::subscriptions(&machine()).await)
        .unwrap_or_default()
        .into_iter()
        .map(|s| AzureSubscription { name: s.name, id: s.id, is_default: s.is_default })
        .collect()
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
    std::env::var_os(oci::CONFIG_FILE_VAR).map(std::path::PathBuf::from).or_else(|| crate::platform::home_dir().map(|h| h.join(".oci").join("config")))
}

/// The config file at `path`; not configured when there is none or it can't be read.
fn read_oci_config(path: Option<&std::path::Path>) -> OciConfig {
    match path.and_then(oci::config) {
        Some(c) => OciConfig { configured: true, tenancy: c.tenancy, region: c.region },
        None => OciConfig::default(),
    }
}

/// The active gcloud account email, or None if the CLI isn't signed in. Reliable even when
/// `projects list` is empty or the Resource Manager API is off, so the UI can show "signed in".
/// An account whose sign-in expired (Workspace reauthentication) still shows as active in
/// `auth list` while every call fails: it counts as signed out, so the UI offers to sign in
/// again instead of showing an empty billing list. So does one whose application-default
/// sign-in, the one Terraform uses, ran out on its own: a launch asks for that one first, and
/// would be refused.
pub async fn gcp_account() -> Option<String> {
    let cx = machine();
    let email = answered("the active Google Cloud account", gcp::active_account(&cx).await)??;
    answered("a Google Cloud access token", gcp::access_token(&cx).await)?;
    answered("Google Cloud's application-default sign-in", gcp::signed_in(&cx).await)?;
    Some(email)
}

/// The GCP billing accounts the signed-in user can see (`gcloud billing accounts list`), so they
/// pick one for the per-lab projects. Empty when the CLI is missing or not signed in, or fails
/// (logged).
pub async fn gcp_billing_accounts() -> Vec<GcpBillingAccount> {
    answered("the Google Cloud billing accounts", gcp::billing_accounts(&machine()).await)
        .unwrap_or_default()
        .into_iter()
        .map(|b| GcpBillingAccount { id: b.id, name: b.name, open: b.open })
        .collect()
}

/// The GCP organizations the signed-in user belongs to (`gcloud organizations list`). Empty for
/// a personal / no-org account, so the user can create projects without a parent; and when the CLI
/// fails (logged).
pub async fn gcp_organizations() -> Vec<GcpOrganization> {
    answered("the Google Cloud organizations", gcp::organizations(&machine()).await)
        .unwrap_or_default()
        .into_iter()
        .map(|o| GcpOrganization { id: o.id, name: o.name })
        .collect()
}

/// Signs in to AWS in the browser (`aws login`, AWS CLI >= 2.32.0): console credentials for
/// root / IAM / federation, temporary credentials for up to 12 hours, no SSO setup needed.
/// Streams the CLI output.
pub async fn aws_login(profile: Option<String>, on_line: impl FnMut(String)) -> Result<()> {
    sign_in(&cloud::SignIn::Aws { profile }, on_line).await
}

#[cfg(test)]
mod tests;
