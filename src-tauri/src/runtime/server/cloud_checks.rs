//! "Test connection" for cloud accounts: a checklist per cloud (credentials, what a launch
//! needs, Terraform), each failure worded as the thing to fix.

use std::time::{Duration, Instant};

use crate::platform::exec::{CLI_TIMEOUT, run_cli, run_env_timed};

use super::budget::{budget_unverifiable_message, month_to_date_cost};
use super::checks::{Check, TestResult};
use super::contract::terraform_env;
use super::profile::HostProfile;
use super::sign_in::{AWS_WHO_AM_I, sign_in_question};
use crate::error::{Error, Result};

/// The program and arguments of an aws subcommand the way this host connects: the host CLI
/// (CLI credentials), or the official CLI container with the keys in its environment.
fn aws_invocation<'a>(h: &HostProfile, sub: &[&'a str]) -> (&'static str, Vec<&'a str>) {
    if h.use_cli_creds {
        ("aws", sub.to_vec())
    } else {
        let mut args = vec!["run", "--rm", "-e", "AWS_ACCESS_KEY_ID", "-e", "AWS_SECRET_ACCESS_KEY", "-e", "AWS_REGION", "amazon/aws-cli:2.37.9"];
        args.extend_from_slice(sub);
        ("docker", args)
    }
}

/// How long an aws call in the CLI container may take: the first one pulls the image.
const AWS_IMAGE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Runs an aws subcommand the way this host connects: the host CLI (CLI credentials), or the
/// official CLI container with the keys in its environment (Docker is the floor). Given up after
/// a minute (five in the container, whose image the first call pulls).
pub(super) async fn aws_cmd(h: &HostProfile, env: &[(String, String)], sub: &[&str]) -> Result<String> {
    let (program, args) = aws_invocation(h, sub);
    let limit = if program == "docker" { AWS_IMAGE_TIMEOUT } else { CLI_TIMEOUT };
    run_env_timed(program, &args, None, env, limit).await
}

/// The last line of a tool's output (where CLIs put the reason).
pub(super) fn last_line(s: &str) -> &str {
    s.lines().last().unwrap_or_default()
}

/// The detail of a CLI check the cloud refused: `refused` when stderr carries one of `markers`
/// (signed out, no access), else "`<cloud>` check failed: `<last line>`".
fn refusal(stderr: &str, markers: &[&str], refused: &str, cloud: &str) -> String {
    if markers.iter().any(|m| stderr.contains(m)) { refused.to_string() } else { format!("{cloud} check failed: {}", last_line(stderr)) }
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
fn terraform_outcome(r: Result<String>) -> Check {
    match r {
        Ok(_) => Check::ok("Terraform", "Installed. Labs are provisioned with it."),
        Err(Error::ToolMissing { .. }) => {
            Check::fail("Terraform", "Not installed. Every cloud lab is provisioned with Terraform, so install it before launching.")
        }
        Err(e) => Check::warn("Terraform", format!("Couldn't check Terraform ({}).", last_line(&e.to_string()))),
    }
}

/// Checks a `terraform` binary is on PATH. Every cloud lab is provisioned with it, so a missing
/// Terraform fails every launch; this catches it before the user tries. Shared by all clouds.
async fn terraform_check() -> Check {
    terraform_outcome(run_cli("terraform", &["version"]).await)
}

/// AWS `sts get-caller-identity`: the identity's ARN, or why there is none.
fn aws_credentials(r: Result<String>) -> First<String> {
    match r {
        Ok(a) => {
            let arn = a.trim().to_string();
            First::Pass(Check::ok("Credentials", format!("Signed in as {arn}.")), arn)
        }
        Err(Error::CommandFailed { stderr, .. }) => {
            let markers = ["InvalidClientTokenId", "SignatureDoesNotMatch", "AccessDenied", "Unable to locate credentials", "sso"];
            First::Refused(Check::fail("Credentials", refusal(&stderr, &markers, "AWS rejected these credentials, or the profile isn't signed in.", "AWS")))
        }
        Err(e) => First::Unreachable(Check::fail("Credentials", format!("Couldn't reach AWS: {e}"))),
    }
}

/// The SSM public parameter naming the latest Amazon Linux 2023 image (`x86_64`, for the probe's
/// `t3.micro`) in the account's region: a real, public image the dry run is checked against.
const LATEST_AMAZON_LINUX: &str = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64";

/// A well-formed image id no account has: AWS answers it with `InvalidAMIID.NotFound`, which it
/// only says once the request is authorized. Used when the public parameter can't be read
/// (`ssm:GetParameter` denied, SSM unreachable). Not a 17-character made-up id: AWS reads those
/// as malformed before it looks at the permission at all.
const UNKNOWN_IMAGE: &str = "ami-12345678";

/// The image the launch probe dry-runs: the one SSM printed when it is an image id, else the
/// unknown well-formed one.
fn probe_image(latest: Result<String>) -> String {
    match latest {
        Ok(out) if out.trim().starts_with("ami-") && out.trim().len() > 4 && !out.trim().contains(char::is_whitespace) => out.trim().to_string(),
        _ => UNKNOWN_IMAGE.to_string(),
    }
}

/// The error code of a failed AWS CLI call: `Code` in "An error occurred (Code) when calling…".
fn aws_error_code(stderr: &str) -> Option<&str> {
    let start = stderr.find("An error occurred (")? + "An error occurred (".len();
    let end = stderr[start..].find(')')?;
    Some(&stderr[start..start + end])
}

/// Whether EC2 raises this error before it authorizes the request, so it says nothing about
/// `ec2:RunInstances`: bad or expired credentials, throttling, a malformed or missing parameter,
/// AWS itself, an account not yet usable. Every other code comes after authorization.
fn before_authorization(code: &str) -> bool {
    const BEFORE: &[&str] = &[
        "AuthFailure",
        "ExpiredToken",
        "InvalidClientTokenId",
        "SignatureDoesNotMatch",
        "IncompleteSignature",
        "RequestExpired",
        "RequestLimitExceeded",
        "Throttling",
        "ServiceUnavailable",
        "InternalError",
        "InternalFailure",
        "Unavailable",
        "ValidationError",
        "MissingParameter",
        "InvalidParameter",
        "UnknownParameter",
        "OptInRequired",
        "PendingVerification",
        "AccessDenied",
        "Blocked",
    ];
    BEFORE.iter().any(|b| code.starts_with(b)) || code.ends_with(".Malformed")
}

/// AWS `ec2 run-instances --dry-run` against the probe image: `DryRunOperation` (how AWS says
/// "would have succeeded") passes; so does any error AWS only raises once the request is
/// authorized (`InvalidAMIID.NotFound` for the unknown image, no default VPC, …), since the
/// permission was checked first. `UnauthorizedOperation` fails. An error raised before
/// authorization (throttling, an expired token, a malformed parameter, the network) is a warning
/// with the reason, never a green line.
fn launch_permission(dry: &Result<String>, arn: &str) -> Check {
    const NAME: &str = "Launch permission";
    let unconfirmed = |why: String| Check::warn(NAME, format!("Couldn't confirm EC2 launches are allowed: {why}"));
    match dry {
        Err(Error::CommandFailed { stderr, .. }) => match aws_error_code(stderr) {
            Some("DryRunOperation") => Check::ok(NAME, "Able to launch EC2 instances."),
            Some("UnauthorizedOperation") => Check::fail(NAME, format!("{arn} can't launch EC2 (ec2:RunInstances is denied). Add EC2 permissions to it.")),
            Some(code) if before_authorization(code) => unconfirmed(last_line(stderr).to_string()),
            Some(code) => Check::ok(
                NAME,
                format!("Able to launch EC2 instances (ec2:RunInstances is allowed; the dry run then stopped at {code}, about its probe image, not a lab's)."),
            ),
            None => unconfirmed(last_line(stderr).to_string()),
        },
        Err(e) => unconfirmed(e.to_string()),
        Ok(_) => unconfirmed("the dry run gave no answer.".to_string()),
    }
}

/// The budget line of the AWS checklist: this month's spend against `limit`.
fn budget_outcome(limit: f64, spent: Result<f64>) -> Check {
    match spent {
        Ok(spent) if spent >= limit => Check::fail("Budget", format!("Over budget: ${spent:.2} of ${limit:.0} this month. New labs here are blocked.")),
        Ok(spent) => Check::ok("Budget", format!("${spent:.2} of ${limit:.0} spent this month.")),
        Err(e) => {
            Check::warn("Budget", format!("Set to ${limit:.0}/mo, but this month's spend couldn't be read: {}", budget_unverifiable_message(&e.to_string())))
        }
    }
}

/// AWS: a checklist run before launch. Verifies the credentials (`sts get-caller-identity`), that
/// the identity can launch EC2 (`ec2 run-instances --dry-run` against the region's latest Amazon
/// Linux image, which creates nothing), Terraform, and the monthly budget when one is set.
pub(super) async fn test_aws(h: &HostProfile, password: &str) -> TestResult {
    let started = Instant::now();
    let env = terraform_env(h, password);
    let mut checks: Vec<Check> = Vec::new();

    // 1) Credentials: the question a launch asks (see `sign_in_question`).
    let identity = aws_cmd(h, &env, &AWS_WHO_AM_I).await;
    let arn = match aws_credentials(identity).settle(&mut checks, started) {
        Ok(arn) => arn,
        Err(done) => return done,
    };

    // 2) Launch permission. A dry run creates nothing; it checks ec2:RunInstances against a real
    //    public image of the region (a made-up id is refused before the permission is looked at).
    let image = probe_image(aws_cmd(h, &env, &["ssm", "get-parameter", "--name", LATEST_AMAZON_LINUX, "--query", "Parameter.Value", "--output", "text"]).await);
    let dry = aws_cmd(h, &env, &["ec2", "run-instances", "--dry-run", "--instance-type", "t3.micro", "--image-id", &image, "--output", "text"]).await;
    checks.push(launch_permission(&dry, &arn));

    // 3) Terraform.
    checks.push(terraform_check().await);

    // 4) Budget, only when one is set.
    if let Some(limit) = h.monthly_limit.filter(|v| *v > 0.0) {
        checks.push(budget_outcome(limit, month_to_date_cost(h, password).await));
    }

    TestResult::timed(checks, started)
}

/// Azure `az account show`: the subscription's name, or why it can't be used.
fn azure_subscription(r: Result<String>) -> First<()> {
    match r {
        Ok(name) => First::Pass(Check::ok("Subscription", format!("Signed in to subscription \"{}\".", name.trim())), ()),
        Err(Error::CommandFailed { stderr, .. }) => {
            let markers = ["az login", "not logged in", "AADSTS", "was not found"];
            let refused = "Not signed in to Azure, or no access to that subscription. Sign in and check the subscription id.";
            First::Refused(Check::fail("Subscription", refusal(&stderr, &markers, refused, "Azure")))
        }
        Err(e) => First::Unreachable(Check::fail("Azure CLI", format!("Couldn't run the Azure CLI: {e}"))),
    }
}

/// Azure: `az account show` for the subscription confirms the CLI is signed in and the
/// subscription is reachable (az has no cheap VM-create dry-run like AWS); then whether the
/// region has room for the lab's VM size, and Terraform.
pub(super) async fn test_azure(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let args = ["account", "show", "--subscription", h.cloud_account().unwrap_or_default(), "--query", "name", "--output", "tsv"];
    if let Err(done) = azure_subscription(run_cli("az", &args).await).settle(&mut checks, started) {
        return done;
    }
    checks.push(super::azure_size::capacity_check(h).await);
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

/// GCP `gcloud billing accounts describe`: the billing account's name, or why it can't be seen.
fn gcp_billing(r: Result<String>) -> First<()> {
    match r {
        Ok(name) => First::Pass(Check::ok("Billing account", format!("Signed in; billing account \"{}\".", name.trim())), ()),
        Err(Error::CommandFailed { stderr, .. }) => {
            let markers = ["gcloud auth", "credentials", "does not have permission", "was not found", "PERMISSION_DENIED", "Permission denied"];
            let refused = "Not signed in to Google Cloud, or no access to that billing account. Sign in and check the billing account id.";
            First::Refused(Check::fail("Billing account", refusal(&stderr, &markers, refused, "GCP")))
        }
        Err(e) => First::Unreachable(Check::fail("gcloud CLI", format!("Couldn't run the gcloud CLI: {e}"))),
    }
}

/// GCP's sign-in as a launch asks for it ([`sign_in_question`]): the application-default token
/// its Terraform provider signs in with, or why there is none.
fn gcp_sign_in(r: Result<String>) -> First<()> {
    match r {
        Ok(_) => First::Pass(Check::ok("Sign-in", "Signed in for Terraform (application-default credentials)."), ()),
        Err(Error::CommandFailed { stderr, .. }) => First::Refused(Check::fail(
            "Sign-in",
            format!(
                "Terraform's sign-in to Google Cloud (application-default credentials) has expired or is missing. Sign in again. ({})",
                last_line(&stderr).trim()
            ),
        )),
        Err(e) => First::Unreachable(Check::fail("gcloud CLI", format!("Couldn't run the gcloud CLI: {e}"))),
    }
}

/// GCP: the sign-in a launch asks for first, then `gcloud billing accounts describe` confirms
/// gcloud is signed in and can see the billing account the labs project is linked to. (No cheap
/// VM-create dry-run like AWS.)
pub(super) async fn test_gcp(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    if let Some((program, question)) = sign_in_question(crate::runtime::providers::Provider::Gcp)
        && let Err(done) = gcp_sign_in(run_cli(program, question).await).settle(&mut checks, started)
    {
        return done;
    }
    let args = ["billing", "accounts", "describe", h.cloud_account().unwrap_or_default(), "--format", "value(displayName)"];
    if let Err(done) = gcp_billing(run_cli("gcloud", &args).await).settle(&mut checks, started) {
        return done;
    }
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

/// A token API's answer to its `account` endpoint: the HTTP status, or why it couldn't be reached.
fn token_outcome(cloud: &str, instances: &str, answer: std::result::Result<reqwest::StatusCode, String>) -> First<()> {
    match answer {
        Ok(status) if status.is_success() => First::Pass(Check::ok("API token", format!("Token is valid. Labs run as {instances} in this account.")), ()),
        Ok(status) if status == reqwest::StatusCode::UNAUTHORIZED => {
            First::Refused(Check::fail("API token", format!("{cloud} rejected this token. Create a new one with read/write scope.")))
        }
        Ok(status) => First::Refused(Check::fail("API token", format!("{cloud} API returned {status}."))),
        Err(e) => First::Unreachable(Check::fail("API token", format!("Can't reach {cloud}: {e}"))),
    }
}

/// A token-authenticated cloud (`DigitalOcean`, Linode): the API token is valid when its
/// `account` endpoint returns 2xx. `instances` is what labs run as there.
async fn test_token_api(cloud: &str, account_url: &str, instances: &str, token: &str) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let answer = match crate::platform::http::api_client() {
        Ok(client) => client.get(account_url).timeout(Duration::from_secs(15)).bearer_auth(token).send().await.map(|r| r.status()).map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(done) = token_outcome(cloud, instances, answer).settle(&mut checks, started) {
        return done;
    }
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

pub(super) async fn test_digitalocean(token: &str) -> TestResult {
    test_token_api("DigitalOcean", "https://api.digitalocean.com/v2/account", "droplets", token).await
}

pub(super) async fn test_linode(token: &str) -> TestResult {
    test_token_api("Linode", "https://api.linode.com/v4/account", "Linodes", token).await
}

/// OCI `iam compartment get`: the compartment's name, or why it couldn't be read. A missing CLI
/// isn't fatal (Terraform only needs ~/.oci/config).
fn oci_compartment(r: Result<String>) -> First<()> {
    match r {
        Ok(name) => First::Pass(Check::ok("Compartment", format!("Reached compartment \"{}\". Labs run in your tenancy.", name.trim())), ()),
        // Terraform only needs ~/.oci/config, so a missing CLI isn't fatal; it just can't verify here.
        Err(Error::ToolMissing { .. }) => First::Pass(
            Check::warn("Compartment", "Couldn't verify here (the OCI CLI isn't installed). Terraform will use ~/.oci/config when you launch a lab."),
            (),
        ),
        Err(Error::CommandFailed { stderr, .. }) => {
            let markers = ["NotAuthenticated", "NotAuthorizedOrNotFound", "config", "private key", "401"];
            let refused = "OCI rejected the request. Check ~/.oci/config (API key) and the compartment OCID.";
            First::Refused(Check::fail("Compartment", refusal(&stderr, &markers, refused, "OCI")))
        }
        Err(e) => First::Unreachable(Check::fail("Compartment", format!("OCI check failed: {e}"))),
    }
}

/// OCI: if the OCI CLI is installed, a signed `iam compartment get` confirms ~/.oci/config +
/// the compartment. Terraform itself only needs ~/.oci/config, so a missing CLI isn't fatal.
pub(super) async fn test_oci(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let answer =
        run_cli("oci", &["iam", "compartment", "get", "--compartment-id", h.cloud_account().unwrap_or_default(), "--query", "data.name", "--raw-output"]).await;
    if let Err(done) = oci_compartment(answer).settle(&mut checks, started) {
        return done;
    }
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

#[cfg(test)]
mod tests;
