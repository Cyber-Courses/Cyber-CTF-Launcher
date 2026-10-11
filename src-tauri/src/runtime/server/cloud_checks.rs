//! "Test connection" for cloud accounts: a checklist per cloud (credentials, what a launch
//! needs, Terraform), each failure worded as the thing to fix.

use std::time::Instant;

use super::budget::{budget_unverifiable_message, month_to_date_cost};
use super::checks::{Check, TestResult};
use super::contract::terraform_env;
use super::profile::HostProfile;
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

/// Runs an aws subcommand the way this host connects: the host CLI (CLI credentials), or the
/// official CLI container with the keys in its environment (Docker is the floor).
pub(super) async fn aws_cmd(h: &HostProfile, env: &[(String, String)], sub: &[&str]) -> Result<String> {
    let (program, args) = aws_invocation(h, sub);
    crate::exec::run_env(program, &args, None, env).await
}

/// The last line of a tool's output (where CLIs put the reason).
pub(super) fn last_line(s: &str) -> &str {
    s.lines().last().unwrap_or_default()
}

/// The detail of a CLI check the cloud refused: `refused` when stderr carries one of `markers`
/// (signed out, no access), else "<cloud> check failed: <last line>".
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
    terraform_outcome(crate::exec::run("terraform", &["version"], None).await)
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

/// AWS `ec2 run-instances --dry-run`: only an explicit `UnauthorizedOperation` is a refusal (a
/// dry run that would have succeeded fails too, with `DryRunOperation`).
fn launch_permission(dry: &Result<String>, arn: &str) -> Check {
    if matches!(dry, Err(Error::CommandFailed { stderr, .. }) if stderr.contains("UnauthorizedOperation")) {
        Check::fail("Launch permission", format!("{arn} can't launch EC2 (ec2:RunInstances is denied). Add EC2 permissions to it."))
    } else {
        Check::ok("Launch permission", "Able to launch EC2 instances.")
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
/// the identity can launch EC2 (`ec2 run-instances --dry-run`, which creates nothing), Terraform,
/// and the monthly budget when one is set.
pub(super) async fn test_aws(h: &HostProfile, password: &str) -> TestResult {
    let started = Instant::now();
    let env = terraform_env(h, password);
    let mut checks: Vec<Check> = Vec::new();

    // 1) Credentials.
    let identity = aws_cmd(h, &env, &["sts", "get-caller-identity", "--query", "Arn", "--output", "text"]).await;
    let arn = match aws_credentials(identity).settle(&mut checks, started) {
        Ok(arn) => arn,
        Err(done) => return done,
    };

    // 2) Launch permission. A dry run creates nothing; it only checks ec2:RunInstances.
    let dry =
        aws_cmd(h, &env, &["ec2", "run-instances", "--dry-run", "--instance-type", "t3.micro", "--image-id", "ami-00000000000000000", "--output", "text"])
            .await;
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
/// subscription is reachable. (az has no cheap VM-create dry-run like AWS.)
pub(super) async fn test_azure(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let args = ["account", "show", "--subscription", h.username.as_str(), "--query", "name", "--output", "tsv"];
    if let Err(done) = azure_subscription(crate::exec::run("az", &args, None).await).settle(&mut checks, started) {
        return done;
    }
    checks.push(azure_capacity_check(h).await);
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

/// Free cores in a VM family's quota, from `az vm list-usage` (numbers come as strings or ints).
fn free_cores(usage: &[serde_json::Value], family: &str) -> Option<i64> {
    usage.iter().find(|x| x["name"]["value"].as_str().map(str::to_lowercase).as_deref() == Some(family)).map(|x| {
        let n = |v: &serde_json::Value| v.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| v.as_i64()).unwrap_or(0);
        n(&x["limit"]) - n(&x["currentValue"])
    })
}

/// The lab VM size in `az vm list-skus` output: its family (lowercase) when the subscription
/// may create it in `region`, else the failed check.
fn size_family(skus_json: Option<&str>, size: &str, region: &str) -> std::result::Result<String, Check> {
    let skus: Vec<serde_json::Value> = skus_json.and_then(|o| serde_json::from_str(o).ok()).unwrap_or_default();
    let Some(entry) = skus.iter().find(|k| k["name"].as_str() == Some(size)) else {
        return Err(Check::fail("VM size", format!("{size} isn't offered in {region}. Try the region swedencentral, which accepts new subscriptions.")));
    };
    if entry["restrictions"].as_array().is_some_and(|r| !r.is_empty()) {
        return Err(Check::fail("VM size", format!("{size} isn't available to this subscription in {region}. Try the region swedencentral.")));
    }
    Ok(entry["family"].as_str().unwrap_or_default().to_lowercase())
}

/// The VM size check from the family's quota in `az vm list-usage` output.
fn quota_check(usage_json: Option<&str>, family: &str, size: &str, region: &str) -> Check {
    let free = usage_json.and_then(|o| serde_json::from_str::<Vec<serde_json::Value>>(o).ok()).and_then(|u| free_cores(&u, family));
    match free {
        Some(f) if f >= 2 => Check::ok("VM size", format!("{size} is available in {region} ({f} cores of quota free).")),
        Some(_) => Check::fail(
            "VM size",
            format!("No core quota left for {size} in {region}. Request more in the Azure portal (Quotas), or try the region swedencentral."),
        ),
        None => Check::warn("VM size", format!("Couldn't read the core quota for {size} in {region}; a launch may still fail on it.")),
    }
}

/// Azure: can this subscription create the lab VM size in the region? New subscriptions often
/// can't: a region refuses new customers, a size isn't offered to the subscription, or its
/// family has no core quota. All seen on real runs; each makes a launch fail late.
async fn azure_capacity_check(h: &HostProfile) -> Check {
    // The size labs use: the one set on the account, else Isoloom's default for a small lab.
    let size = h.datastore.clone().unwrap_or_else(|| "Standard_D2als_v6".into());
    let region = h.host.as_str();
    let sku = crate::exec::run(
        "az",
        &["vm", "list-skus", "--subscription", &h.username, "-l", region, "--size", &size, "--resource-type", "virtualMachines", "-o", "json"],
        None,
    )
    .await;
    let family = match size_family(sku.ok().as_deref(), &size, region) {
        Ok(family) => family,
        Err(check) => return check,
    };
    let usage = crate::exec::run("az", &["vm", "list-usage", "--subscription", &h.username, "-l", region, "-o", "json"], None).await;
    quota_check(usage.ok().as_deref(), &family, &size, region)
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

/// GCP: `gcloud billing accounts describe` confirms gcloud is signed in and can see the billing
/// account the labs project is linked to. (No cheap VM-create dry-run like AWS.)
pub(super) async fn test_gcp(h: &HostProfile) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let args = ["billing", "accounts", "describe", h.username.as_str(), "--format", "value(displayName)"];
    if let Err(done) = gcp_billing(crate::exec::run("gcloud", &args, None).await).settle(&mut checks, started) {
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

/// A token-authenticated cloud (DigitalOcean, Linode): the API token is valid when its
/// `account` endpoint returns 2xx. `instances` is what labs run as there.
async fn test_token_api(cloud: &str, account_url: &str, instances: &str, token: &str) -> TestResult {
    let started = Instant::now();
    let mut checks: Vec<Check> = Vec::new();
    let answer = match crate::http::api_client() {
        Ok(client) => client.get(account_url).bearer_auth(token).send().await.map(|r| r.status()).map_err(|e| e.to_string()),
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
        crate::exec::run("oci", &["iam", "compartment", "get", "--compartment-id", h.username.as_str(), "--query", "data.name", "--raw-output"], None).await;
    if let Err(done) = oci_compartment(answer).settle(&mut checks, started) {
        return done;
    }
    checks.push(terraform_check().await);
    TestResult::timed(checks, started)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::providers::Provider;
    use crate::runtime::server::checks::CheckState;
    use crate::runtime::server::profile::tests::profile;

    fn failed(stderr: &str) -> Error {
        Error::CommandFailed { command: "cli".into(), stderr: stderr.into() }
    }

    fn missing() -> Error {
        Error::ToolMissing { tool: "cli" }
    }

    /// The check of a first step, and which way it went: "pass", "refused" or "unreachable".
    fn first<T>(f: First<T>) -> (&'static str, Check) {
        match f {
            First::Pass(c, _) => ("pass", c),
            First::Refused(c) => ("refused", c),
            First::Unreachable(c) => ("unreachable", c),
        }
    }

    #[test]
    fn refusal_names_the_fix_or_quotes_the_cli() {
        let markers = ["az login", "AADSTS"];
        assert_eq!(refusal("ERROR: Please run 'az login' to setup account.", &markers, "Sign in.", "Azure"), "Sign in.");
        assert_eq!(refusal("WARNING: x\nERROR: quota exceeded", &markers, "Sign in.", "Azure"), "Azure check failed: ERROR: quota exceeded");
        assert_eq!(refusal("", &markers, "Sign in.", "OCI"), "OCI check failed: ");
    }

    #[test]
    fn free_cores_reads_string_or_number_quotas() {
        let usage = serde_json::json!([
            {"name": {"value": "standardDASv6Family"}, "limit": "10", "currentValue": "4"},
            {"name": {"value": "cores"}, "limit": 20, "currentValue": 19}
        ]);
        let usage = usage.as_array().unwrap();
        assert_eq!(free_cores(usage, "standarddasv6family"), Some(6));
        assert_eq!(free_cores(usage, "cores"), Some(1));
        assert_eq!(free_cores(usage, "other"), None);
    }

    #[test]
    fn aws_runs_on_the_host_cli_or_in_the_cli_container() {
        let cli = HostProfile { use_cli_creds: true, ..profile(Provider::Aws) };
        assert_eq!(aws_invocation(&cli, &["sts", "get-caller-identity"]), ("aws", vec!["sts", "get-caller-identity"]));
        let (program, args) = aws_invocation(&profile(Provider::Aws), &["sts", "get-caller-identity"]);
        assert_eq!(program, "docker");
        assert_eq!(args[..2], ["run", "--rm"]);
        assert!(args.contains(&"AWS_SECRET_ACCESS_KEY"));
        assert!(args.iter().any(|a| a.starts_with("amazon/aws-cli:")));
        assert_eq!(args[args.len() - 2..], ["sts", "get-caller-identity"]);
    }

    #[test]
    fn last_line_is_where_the_reason_is() {
        assert_eq!(last_line("a\nb\nreason"), "reason");
        assert_eq!(last_line(""), "");
    }

    #[test]
    fn terraform_is_ok_missing_or_unknown() {
        assert_eq!(terraform_outcome(Ok("Terraform v1.9".into())).state, CheckState::Ok);
        let c = terraform_outcome(Err(missing()));
        assert_eq!(c.state, CheckState::Fail);
        assert!(c.detail.starts_with("Not installed."), "{}", c.detail);
        let c = terraform_outcome(Err(failed("line one\nbroken plugin cache")));
        assert_eq!(c.state, CheckState::Warn);
        assert_eq!(c.detail, "Couldn't check Terraform (broken plugin cache).");
    }

    #[test]
    fn aws_credentials_pass_with_the_arn() {
        match aws_credentials(Ok("arn:aws:iam::123:user/alex\n".into())) {
            First::Pass(c, arn) => {
                assert_eq!(arn, "arn:aws:iam::123:user/alex");
                assert_eq!(c.detail, "Signed in as arn:aws:iam::123:user/alex.");
            }
            _ => panic!("should pass"),
        }
        let (way, c) = first(aws_credentials(Err(failed("An error occurred (InvalidClientTokenId)"))));
        assert_eq!((way, c.detail.as_str()), ("refused", "AWS rejected these credentials, or the profile isn't signed in."));
        let (way, c) = first(aws_credentials(Err(failed("throttled\nRate exceeded"))));
        assert_eq!((way, c.detail.as_str()), ("refused", "AWS check failed: Rate exceeded"));
        let (way, c) = first(aws_credentials(Err(missing())));
        assert_eq!(way, "unreachable");
        assert!(c.detail.starts_with("Couldn't reach AWS: "), "{}", c.detail);
    }

    #[test]
    fn only_an_unauthorized_dry_run_denies_launching() {
        let denied = Err(failed("An error occurred (UnauthorizedOperation) when calling RunInstances"));
        let c = launch_permission(&denied, "arn:x");
        assert_eq!(c.state, CheckState::Fail);
        assert!(c.detail.starts_with("arn:x can't launch EC2"), "{}", c.detail);
        // DryRunOperation is how AWS says "would have succeeded".
        assert_eq!(launch_permission(&Err(failed("(DryRunOperation) Request would have succeeded")), "arn:x").state, CheckState::Ok);
        assert_eq!(launch_permission(&Ok(String::new()), "arn:x").state, CheckState::Ok);
    }

    #[test]
    fn the_budget_line_compares_spend_with_the_limit() {
        let c = budget_outcome(50.0, Ok(12.345));
        assert_eq!((c.state, c.detail.as_str()), (CheckState::Ok, "$12.35 of $50 spent this month."));
        let c = budget_outcome(50.0, Ok(50.0));
        assert_eq!(c.state, CheckState::Fail);
        assert_eq!(c.detail, "Over budget: $50.00 of $50 this month. New labs here are blocked.");
        let c = budget_outcome(50.0, Err(Error::Invalid("Unable to locate credentials".into())));
        assert_eq!(c.state, CheckState::Warn);
        assert!(c.detail.ends_with("the AWS session has expired or isn't signed in."), "{}", c.detail);
    }

    #[test]
    fn azure_sign_in_outcomes() {
        let (way, c) = first(azure_subscription(Ok("Pay-As-You-Go\n".into())));
        assert_eq!((way, c.detail.as_str()), ("pass", "Signed in to subscription \"Pay-As-You-Go\"."));
        let (way, c) = first(azure_subscription(Err(failed("ERROR: Please run 'az login' to setup account."))));
        assert_eq!(way, "refused");
        assert!(c.detail.starts_with("Not signed in to Azure"), "{}", c.detail);
        let (way, c) = first(azure_subscription(Err(missing())));
        assert_eq!((way, c.name.as_str()), ("unreachable", "Azure CLI"));
    }

    #[test]
    fn azure_sizes_must_be_offered_unrestricted_and_have_quota() {
        let skus = r#"[{"name": "Standard_D2als_v6", "family": "standardDALSv6Family", "restrictions": []}]"#;
        assert_eq!(size_family(Some(skus), "Standard_D2als_v6", "westeurope").ok().unwrap(), "standarddalsv6family");
        let c = size_family(Some(skus), "Standard_B1s", "westeurope").err().unwrap();
        assert!(c.detail.starts_with("Standard_B1s isn't offered in westeurope."), "{}", c.detail);
        // The CLI failing (None) or printing garbage reads as "not offered".
        assert!(size_family(None, "Standard_D2als_v6", "westeurope").is_err());
        assert!(size_family(Some("oops"), "Standard_D2als_v6", "westeurope").is_err());
        let restricted = r#"[{"name": "Standard_D2als_v6", "family": "f", "restrictions": [{"type": "Location"}]}]"#;
        let c = size_family(Some(restricted), "Standard_D2als_v6", "westeurope").err().unwrap();
        assert!(c.detail.contains("isn't available to this subscription"), "{}", c.detail);

        let usage = r#"[{"name": {"value": "standardDALSv6Family"}, "limit": "10", "currentValue": "4"}]"#;
        let c = quota_check(Some(usage), "standarddalsv6family", "Standard_D2als_v6", "westeurope");
        assert_eq!((c.state, c.detail.as_str()), (CheckState::Ok, "Standard_D2als_v6 is available in westeurope (6 cores of quota free)."));
        let full = r#"[{"name": {"value": "standardDALSv6Family"}, "limit": 4, "currentValue": 3}]"#;
        assert_eq!(quota_check(Some(full), "standarddalsv6family", "s", "r").state, CheckState::Fail);
        assert_eq!(quota_check(None, "standarddalsv6family", "s", "r").state, CheckState::Warn);
        assert_eq!(quota_check(Some(usage), "otherfamily", "s", "r").state, CheckState::Warn);
    }

    #[test]
    fn gcp_sign_in_outcomes() {
        let (way, c) = first(gcp_billing(Ok("My billing\n".into())));
        assert_eq!((way, c.detail.as_str()), ("pass", "Signed in; billing account \"My billing\"."));
        let (way, c) = first(gcp_billing(Err(failed("ERROR: (gcloud) PERMISSION_DENIED"))));
        assert_eq!(way, "refused");
        assert!(c.detail.starts_with("Not signed in to Google Cloud"), "{}", c.detail);
        let (way, c) = first(gcp_billing(Err(missing())));
        assert_eq!((way, c.name.as_str()), ("unreachable", "gcloud CLI"));
    }

    #[test]
    fn token_api_outcomes() {
        let (way, c) = first(token_outcome("Linode", "Linodes", Ok(reqwest::StatusCode::OK)));
        assert_eq!((way, c.detail.as_str()), ("pass", "Token is valid. Labs run as Linodes in this account."));
        let (way, c) = first(token_outcome("Linode", "Linodes", Ok(reqwest::StatusCode::UNAUTHORIZED)));
        assert_eq!((way, c.detail.as_str()), ("refused", "Linode rejected this token. Create a new one with read/write scope."));
        let (way, c) = first(token_outcome("DigitalOcean", "droplets", Ok(reqwest::StatusCode::INTERNAL_SERVER_ERROR)));
        assert_eq!((way, c.detail.as_str()), ("refused", "DigitalOcean API returned 500 Internal Server Error."));
        let (way, c) = first(token_outcome("DigitalOcean", "droplets", Err("dns error".into())));
        assert_eq!((way, c.detail.as_str()), ("unreachable", "Can't reach DigitalOcean: dns error"));
    }

    #[test]
    fn oci_outcomes_and_a_missing_cli_only_warns() {
        let (way, c) = first(oci_compartment(Ok("root\n".into())));
        assert_eq!((way, c.state), ("pass", CheckState::Ok));
        assert_eq!(c.detail, "Reached compartment \"root\". Labs run in your tenancy.");
        let (way, c) = first(oci_compartment(Err(missing())));
        assert_eq!((way, c.state), ("pass", CheckState::Warn));
        let (way, c) = first(oci_compartment(Err(failed("ServiceError: NotAuthenticated"))));
        assert_eq!(way, "refused");
        assert!(c.detail.starts_with("OCI rejected the request."), "{}", c.detail);
        let (way, _) = first(oci_compartment(Err(Error::Invalid("boom".into()))));
        assert_eq!(way, "unreachable");
    }

    #[test]
    fn a_settled_first_step_ends_the_checklist_or_goes_on() {
        let started = Instant::now();
        let mut checks = vec![Check::ok("Earlier", "fine")];
        assert_eq!(First::Pass(Check::ok("A", "a"), 7).settle(&mut checks, started).ok(), Some(7));
        assert_eq!(checks.len(), 2);

        let done = First::<()>::Refused(Check::fail("B", "refused")).settle(&mut checks, started).err().unwrap();
        assert!(done.reachable && !done.ok && done.latency_ms.is_some());
        assert_eq!(done.message, "refused");
        assert_eq!(done.checks.len(), 3);
        assert!(checks.is_empty());

        let done = First::<()>::Unreachable(Check::fail("C", "down")).settle(&mut checks, started).err().unwrap();
        assert!(!done.reachable && !done.ok && done.latency_ms.is_none());
        assert_eq!(done.message, "down");
    }
}
