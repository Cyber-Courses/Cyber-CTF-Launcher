//! "Test connection" for cloud accounts: a checklist per cloud (credentials, what a launch
//! needs, Terraform), each failure worded as the thing to fix.

use std::time::Instant;

use isothrum_machine::cloud::{TokenError, aws, azure, digitalocean, gcp, linode, oci};
use isothrum_machine::{Failure, Refusal};

use super::budget::{budget_unverifiable_message, month_to_date_cost};
use super::checks::{Check, TestResult};
use super::profile::HostProfile;
use crate::cloud_cli::{machine, tool_error};
use crate::error::Error;

/// The AWS account a host is: the host CLI's credentials (its profile), or its stored access key
/// (`password`), its calls to the host's region.
pub(super) fn aws_account(h: &HostProfile, password: &str) -> aws::Account {
    let credentials = if h.use_cli_creds {
        aws::Credentials::Cli { profile: h.aws_profile.clone() }
    } else {
        aws::Credentials::Keys { id: h.username.clone(), secret: password.to_owned() }
    };
    aws::Account::new(credentials, Some(h.host.clone()))
}

/// The last line of a tool's output (where CLIs put the reason).
pub(super) fn last_line(s: &str) -> &str {
    s.lines().last().unwrap_or_default()
}

/// What a cloud CLI's failure is to a checklist: the cloud answered and refused (how, when the
/// CLI's words say, and its reason), or the CLI couldn't be asked at all. A CLI that ran out of
/// time counts as a refusal, its reason the timed tool's.
enum Asked {
    /// The cloud refused: how (none for a CLI that ran out of time), and why.
    Refused(Option<Refusal>, String),
    /// The CLI couldn't be run.
    Unreachable(Error),
}

fn asked(failure: &Failure) -> Asked {
    match failure {
        Failure::Refused { refusal, said, .. } => Asked::Refused(Some(*refusal), said.clone()),
        Failure::TimedOut { .. } => match tool_error(failure) {
            Error::CommandFailed { stderr, .. } => Asked::Refused(None, stderr),
            other => Asked::Unreachable(other),
        },
        other => Asked::Unreachable(tool_error(other)),
    }
}

/// A failure's reason, as a check quotes it: what the cloud said, else the launcher's error.
pub(super) fn reason(failure: &Failure) -> String {
    match asked(failure) {
        Asked::Refused(_, said) => said,
        Asked::Unreachable(e) => e.to_string(),
    }
}

/// The detail of a CLI check the cloud refused: `refused` when the CLI's words say it is one of
/// `refusals` (signed out, no access), else "`<cloud>` check failed: `<its reason>`".
fn refusal(how: Option<Refusal>, said: &str, refusals: &[Refusal], refused: &str, cloud: &str) -> String {
    if how.is_some_and(|r| refusals.contains(&r)) { refused.to_string() } else { format!("{cloud} check failed: {}", last_line(said)) }
}

/// The outcome of a checklist's first check (the sign-in): go on, or stop with the result.
enum First<T> {
    /// Passed: the check, and what the rest of the checklist needs from it.
    Pass(Check, T),
    /// The cloud answered and refused: the checklist ends here.
    Refused(Check),
    /// The cloud (or its CLI) couldn't be reached at all.
    Unreachable(Check),
}

impl<T> First<T> {
    /// Adds the check to `checks`; the value to go on with, else the finished result.
    fn settle(self, checks: &mut Vec<Check>, started: Instant) -> std::result::Result<T, TestResult> {
        match self {
            First::Pass(c, v) => {
                checks.push(c);
                Ok(v)
            }
            First::Refused(c) => {
                checks.push(c);
                Err(TestResult::timed(std::mem::take(checks), started))
            }
            First::Unreachable(c) => {
                checks.push(c);
                Err(TestResult::unreachable_checks(std::mem::take(checks)))
            }
        }
    }
}

/// Whether `terraform version` ran.
fn terraform_outcome(r: std::result::Result<String, Failure>) -> Check {
    match r {
        Ok(_) => Check::ok("Terraform", "Installed. Labs are provisioned with it."),
        Err(Failure::Missing { .. }) => {
            Check::fail("Terraform", "Not installed. Every cloud lab is provisioned with Terraform, so install it before launching.")
        }
        Err(e) => Check::warn("Terraform", format!("Couldn't check Terraform ({}).", last_line(&reason(&e)))),
    }
}

/// Checks a `terraform` binary is on PATH. Every cloud lab is provisioned with it, so a missing
/// Terraform fails every launch; this catches it before the user tries. Shared by all clouds.
async fn terraform_check() -> Check {
    terraform_outcome(isothrum_machine::failure::ask(&machine(), isothrum_machine::tools::probes::TERRAFORM).await)
}

/// AWS's identity: the identity's ARN, or why there is none.
fn aws_credentials(r: std::result::Result<String, Failure>) -> First<String> {
    match r {
        Ok(arn) => First::Pass(Check::ok("Credentials", format!("Signed in as {arn}.")), arn),
        Err(e) => match asked(&e) {
            Asked::Refused(how, said) => {
                let refused = "AWS rejected these credentials, or the profile isn't signed in.";
                First::Refused(Check::fail("Credentials", refusal(how, &said, &[Refusal::CredentialsExpired, Refusal::Denied], refused, "AWS")))
            }
            Asked::Unreachable(e) => First::Unreachable(Check::fail("Credentials", format!("Couldn't reach AWS: {e}"))),
        },
    }
}

/// The launch permission's line, from AWS's dry run ([`aws::launch_permission`]): allowed (even
/// when the dry run then stopped at its probe image), denied with the identity, or unconfirmed
/// with the reason, never a green line for an answer that says nothing of the permission.
fn launch_permission(dry: &aws::LaunchPermission, arn: &str) -> Check {
    const NAME: &str = "Launch permission";
    let unconfirmed = |why: String| Check::warn(NAME, format!("Couldn't confirm EC2 launches are allowed: {why}"));
    match dry {
        aws::LaunchPermission::Allowed => Check::ok(NAME, "Able to launch EC2 instances."),
        aws::LaunchPermission::AllowedThen { code } => Check::ok(
            NAME,
            format!("Able to launch EC2 instances (ec2:RunInstances is allowed; the dry run then stopped at {code}, about its probe image, not a lab's)."),
        ),
        aws::LaunchPermission::Denied => Check::fail(NAME, format!("{arn} can't launch EC2 (ec2:RunInstances is denied). Add EC2 permissions to it.")),
        aws::LaunchPermission::Unconfirmed(e) => unconfirmed(last_line(&reason(e)).to_string()),
        _ => unconfirmed("the dry run gave no answer.".to_string()),
    }
}

/// The budget line of the AWS checklist: this month's spend against `limit`.
fn budget_outcome(limit: f64, spent: std::result::Result<f64, Failure>) -> Check {
    match spent {
        Ok(spent) if spent >= limit => Check::fail("Budget", format!("Over budget: ${spent:.2} of ${limit:.0} this month. New labs here are blocked.")),
        Ok(spent) => Check::ok("Budget", format!("${spent:.2} of ${limit:.0} spent this month.")),
        Err(e) => Check::warn("Budget", format!("Set to ${limit:.0}/mo, but this month's spend couldn't be read: {}", budget_unverifiable_message(&e))),
    }
}

/// AWS: a checklist run before launch. Verifies the credentials (who they are, the question a
/// launch asks), that the identity can launch EC2 (a dry run against the region's latest Amazon
/// Linux image, which creates nothing), Terraform, and the monthly budget when one is set.
pub(super) async fn test_aws(h: &HostProfile, password: &str) -> TestResult {
    let started = Instant::now();
    let (cx, account) = (machine(), aws_account(h, password));
    let mut checks: Vec<Check> = Vec::new();

    // 1) Credentials.
    let arn = match aws_credentials(aws::identity(&cx, &account).await).settle(&mut checks, started) {
        Ok(arn) => arn,
        Err(done) => return done,
    };

    // 2) Launch permission: a dry run creates nothing.
    checks.push(launch_permission(&aws::launch_permission(&cx, &account).await, &arn));

    // 3) Terraform.
    checks.push(terraform_check().await);

    // 4) Budget, only when one is set.
    if let Some(limit) = h.monthly_limit.filter(|v| *v > 0.0) {
        checks.push(budget_outcome(limit, month_to_date_cost(h, password).await));
    }

    TestResult::timed(checks, started)
}

/// Azure's subscription: its name, or why it can't be used.
fn azure_subscription(r: std::result::Result<String, Failure>) -> First<()> {
    match r {
        Ok(name) => First::Pass(Check::ok("Subscription", format!("Signed in to subscription \"{name}\".")), ()),
        Err(e) => match asked(&e) {
            Asked::Refused(how, said) => {
                let refused = "Not signed in to Azure, or no access to that subscription. Sign in and check the subscription id.";
                First::Refused(Check::fail("Subscription", refusal(how, &said, &[Refusal::CredentialsExpired, Refusal::NotFound], refused, "Azure")))
            }
            Asked::Unreachable(e) => First::Unreachable(Check::fail("Azure CLI", format!("Couldn't run the Azure CLI: {e}"))),
        },
    }
}

/// Azure: the subscription's name confirms the CLI is signed in and the subscription is reachable
/// (az has no cheap VM-create dry-run like AWS); then whether the region has room for the lab's
/// VM size, and Terraform.
pub(super) async fn test_azure(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let name = azure::subscription_name(&machine(), h.cloud_account().unwrap_or_default()).await;
    if let Err(done) = azure_subscription(name).settle(&mut checks, started) {
        return done;
    }
    checks.push(super::azure_size::capacity_check(h).await);
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

/// GCP's billing account: its name, or why it can't be seen.
fn gcp_billing(r: std::result::Result<String, Failure>) -> First<()> {
    match r {
        Ok(name) => First::Pass(Check::ok("Billing account", format!("Signed in; billing account \"{name}\".")), ()),
        Err(e) => match asked(&e) {
            Asked::Refused(how, said) => {
                let refused = "Not signed in to Google Cloud, or no access to that billing account. Sign in and check the billing account id.";
                let refusals = [Refusal::CredentialsExpired, Refusal::Denied, Refusal::NotFound];
                First::Refused(Check::fail("Billing account", refusal(how, &said, &refusals, refused, "GCP")))
            }
            Asked::Unreachable(e) => First::Unreachable(Check::fail("gcloud CLI", format!("Couldn't run the gcloud CLI: {e}"))),
        },
    }
}

/// GCP's sign-in as a launch asks for it: the application-default token its Terraform provider
/// signs in with, or why there is none.
fn gcp_sign_in(r: std::result::Result<(), Failure>) -> First<()> {
    match r {
        Ok(()) => First::Pass(Check::ok("Sign-in", "Signed in for Terraform (application-default credentials)."), ()),
        Err(e) => match asked(&e) {
            Asked::Refused(_, said) => First::Refused(Check::fail(
                "Sign-in",
                format!(
                    "Terraform's sign-in to Google Cloud (application-default credentials) has expired or is missing. Sign in again. ({})",
                    last_line(&said).trim()
                ),
            )),
            Asked::Unreachable(e) => First::Unreachable(Check::fail("gcloud CLI", format!("Couldn't run the gcloud CLI: {e}"))),
        },
    }
}

/// GCP: the sign-in a launch asks for first, then the billing account's name confirms gcloud is
/// signed in and can see the billing account the labs project is linked to. (No cheap VM-create
/// dry-run like AWS.)
pub(super) async fn test_gcp(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let cx = machine();
    let mut checks: Vec<Check> = Vec::new();
    if let Err(done) = gcp_sign_in(gcp::signed_in(&cx).await).settle(&mut checks, started) {
        return done;
    }
    let name = gcp::billing_account_name(&cx, h.cloud_account().unwrap_or_default()).await;
    if let Err(done) = gcp_billing(name).settle(&mut checks, started) {
        return done;
    }
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

/// A token API's answer to its `account` endpoint: the HTTP status, or why it couldn't be reached.
fn token_outcome(cloud: &str, instances: &str, answer: std::result::Result<(), TokenError>) -> First<()> {
    match answer {
        Ok(()) => First::Pass(Check::ok("API token", format!("Token is valid. Labs run as {instances} in this account.")), ()),
        Err(TokenError::Rejected) => First::Refused(Check::fail("API token", format!("{cloud} rejected this token. Create a new one with read/write scope."))),
        Err(TokenError::Status(code)) => {
            let status = reqwest::StatusCode::from_u16(code).map_or_else(|_| code.to_string(), |s| s.to_string());
            First::Refused(Check::fail("API token", format!("{cloud} API returned {status}.")))
        }
        Err(TokenError::Unreachable(e)) => First::Unreachable(Check::fail("API token", format!("Can't reach {cloud}: {e}"))),
        Err(other) => First::Unreachable(Check::fail("API token", format!("Can't reach {cloud}: {other}"))),
    }
}

/// A token-authenticated cloud (`DigitalOcean`, Linode): `answer` is isothrum-machine's check of
/// the token against the provider's `account` endpoint. `instances` is what labs run as there.
async fn test_token_api(cloud: &str, instances: &str, answer: std::result::Result<(), TokenError>) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    if let Err(done) = token_outcome(cloud, instances, answer).settle(&mut checks, started) {
        return done;
    }
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

pub(super) async fn test_digitalocean(token: &str) -> TestResult {
    test_token_api("DigitalOcean", "droplets", digitalocean::check_token(token).await).await
}

pub(super) async fn test_linode(token: &str) -> TestResult {
    test_token_api("Linode", "Linodes", linode::check_token(token).await).await
}

/// OCI's compartment: its name, or why it couldn't be read. A missing CLI isn't fatal
/// (Terraform only needs ~/.oci/config).
fn oci_compartment(r: std::result::Result<String, Failure>) -> First<()> {
    match r {
        Ok(name) => First::Pass(Check::ok("Compartment", format!("Reached compartment \"{name}\". Labs run in your tenancy.")), ()),
        // Terraform only needs ~/.oci/config, so a missing CLI isn't fatal; it just can't verify here.
        Err(Failure::Missing { .. }) => First::Pass(
            Check::warn("Compartment", "Couldn't verify here (the OCI CLI isn't installed). Terraform will use ~/.oci/config when you launch a lab."),
            (),
        ),
        Err(e) => match asked(&e) {
            Asked::Refused(how, said) => {
                let refused = "OCI rejected the request. Check ~/.oci/config (API key) and the compartment OCID.";
                First::Refused(Check::fail("Compartment", refusal(how, &said, &[Refusal::CredentialsExpired, Refusal::Denied], refused, "OCI")))
            }
            Asked::Unreachable(e) => First::Unreachable(Check::fail("Compartment", format!("OCI check failed: {e}"))),
        },
    }
}

/// OCI: if the OCI CLI is installed, the compartment's name confirms ~/.oci/config and the
/// compartment. Terraform itself only needs ~/.oci/config, so a missing CLI isn't fatal.
pub(super) async fn test_oci(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let answer = oci::compartment_name(&machine(), h.cloud_account().unwrap_or_default()).await;
    if let Err(done) = oci_compartment(answer).settle(&mut checks, started) {
        return done;
    }
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

#[cfg(test)]
mod tests;
