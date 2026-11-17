use super::*;
use crate::runtime::providers::Provider;
use crate::runtime::server::checks::CheckState;
use crate::runtime::server::profile::tests::profile;
use crate::testing::tools::{Reply, Tools, fail, ok};

/// `program` refusing, saying `said`, its refusal read by `read` (the CLI's reader in
/// isothrum-machine).
fn refused(program: &str, read: fn(&str) -> Refusal, said: &str) -> Failure {
    let last = said.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    Failure::Refused { program: program.into(), refusal: read(said), said: last.into() }
}

fn aws_failed(said: &str) -> Failure {
    refused("aws", aws::refusal, said)
}

fn missing() -> Failure {
    Failure::Missing { program: "terraform".into() }
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
    let refusals = [Refusal::CredentialsExpired];
    assert_eq!(refusal(Some(Refusal::CredentialsExpired), "ERROR: Please run 'az login' to setup account.", &refusals, "Sign in.", "Azure"), "Sign in.");
    assert_eq!(refusal(Some(Refusal::Other), "ERROR: quota exceeded", &refusals, "Sign in.", "Azure"), "Azure check failed: ERROR: quota exceeded");
    assert_eq!(refusal(None, "", &refusals, "Sign in.", "OCI"), "OCI check failed: ");
}

#[test]
fn aws_runs_on_the_host_cli_or_in_the_cli_container() {
    let cli = HostProfile { use_cli_creds: true, aws_profile: Some("labs".into()), ..profile(Provider::Aws) };
    let (program, _, env) = aws_account(&cli, "").invocation(&["sts", "get-caller-identity"]);
    assert_eq!(program, "aws");
    assert!(env.contains(&("AWS_PROFILE".to_owned(), "labs".to_owned())));
    let (program, args, env) = aws_account(&profile(Provider::Aws), "s3cret").invocation(&["sts", "get-caller-identity"]);
    assert_eq!(program, "docker");
    assert!(args.iter().any(|a| a.starts_with("amazon/aws-cli:")));
    assert!(env.contains(&("AWS_SECRET_ACCESS_KEY".to_owned(), "s3cret".to_owned())));
}

#[test]
fn a_cli_that_ran_out_of_time_is_quoted_as_a_timed_tool_and_one_missing_cant_be_reached() {
    let stuck = Failure::TimedOut { program: "az".into(), limit: std::time::Duration::from_secs(60) };
    assert!(matches!(asked(&stuck), Asked::Refused(None, said) if said.starts_with("timed out after 60s")));
    assert!(matches!(asked(&missing()), Asked::Unreachable(Error::ToolMissing { tool: "terraform" })));
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
    let c = terraform_outcome(Err(refused("terraform", |_| Refusal::Other, "line one\nbroken plugin cache")));
    assert_eq!(c.state, CheckState::Warn);
    assert_eq!(c.detail, "Couldn't check Terraform (broken plugin cache).");
}

#[test]
fn aws_credentials_pass_with_the_arn() {
    match aws_credentials(Ok("arn:aws:iam::123:user/alex".into())) {
        First::Pass(c, arn) => {
            assert_eq!(arn, "arn:aws:iam::123:user/alex");
            assert_eq!(c.detail, "Signed in as arn:aws:iam::123:user/alex.");
        }
        _ => panic!("should pass"),
    }
    let (way, c) = first(aws_credentials(Err(aws_failed("An error occurred (InvalidClientTokenId)"))));
    assert_eq!((way, c.detail.as_str()), ("refused", "AWS rejected these credentials, or the profile isn't signed in."));
    let (way, c) = first(aws_credentials(Err(aws_failed("throttled\nRate exceeded"))));
    assert_eq!((way, c.detail.as_str()), ("refused", "AWS check failed: Rate exceeded"));
    let (way, c) = first(aws_credentials(Err(missing())));
    assert_eq!(way, "unreachable");
    assert!(c.detail.starts_with("Couldn't reach AWS: "), "{}", c.detail);
}

#[test]
fn each_answer_of_the_dry_run_says_what_it_says_of_the_permission() {
    let c = launch_permission(&aws::LaunchPermission::Denied, "arn:x");
    assert_eq!((c.state, c.detail.as_str()), (CheckState::Fail, "arn:x can't launch EC2 (ec2:RunInstances is denied). Add EC2 permissions to it."));
    let c = launch_permission(&aws::LaunchPermission::Allowed, "arn:x");
    assert_eq!((c.state, c.detail.as_str()), (CheckState::Ok, "Able to launch EC2 instances."));
    let c = launch_permission(&aws::LaunchPermission::AllowedThen { code: "VPCIdNotSpecified".into() }, "arn:x");
    assert_eq!(c.state, CheckState::Ok);
    assert_eq!(
        c.detail,
        "Able to launch EC2 instances (ec2:RunInstances is allowed; the dry run then stopped at VPCIdNotSpecified, about its probe image, not a lab's)."
    );
    let throttled = "An error occurred (RequestLimitExceeded) when calling the RunInstances operation";
    let c = launch_permission(&aws::LaunchPermission::Unconfirmed(aws_failed(throttled)), "arn:x");
    assert_eq!((c.state, c.detail), (CheckState::Warn, format!("Couldn't confirm EC2 launches are allowed: {throttled}")));
    assert_eq!(launch_permission(&aws::LaunchPermission::Unconfirmed(missing()), "arn:x").state, CheckState::Warn);
    assert_eq!(launch_permission(&aws::LaunchPermission::NoAnswer, "arn:x").state, CheckState::Warn);
}

/// The AWS checklist on scripted tools: `dry` answers the dry run, `latest` the SSM lookup.
async fn aws_checklist(latest: Option<Reply>, dry: Option<Reply>) -> (Tools, TestResult) {
    let tools = Tools::strict_rules(&[
        ("sts get-caller-identity", ok("arn:aws:iam::1:user/me\n")),
        ("ssm get-parameter", latest),
        ("ec2 run-instances", dry),
        ("terraform version", ok("Terraform v1.9")),
    ]);
    let result = test_aws(&profile(Provider::Aws), "s3cret").await;
    (tools, result)
}

fn launch_check(result: &TestResult) -> &Check {
    result.checks.iter().find(|c| c.name == "Launch permission").expect("a launch permission check")
}

/// The AWS checklist against the real account the host's AWS CLI is signed in to (its default
/// profile, `CYBERCTF_TEST_AWS_REGION` or eu-west-3), as the Cloud page's Test runs it: the
/// launch permission must come back confirmed, not "couldn't confirm". Creates nothing.
/// `cargo test --lib -- --ignored --nocapture the_real_aws_account_test_confirms_the_launch_permission`
#[tokio::test]
#[ignore = "needs the AWS CLI signed in to a real account"]
async fn the_real_aws_account_test_confirms_the_launch_permission() {
    let h = HostProfile {
        use_cli_creds: true,
        host: std::env::var("CYBERCTF_TEST_AWS_REGION").unwrap_or_else(|_| "eu-west-3".into()),
        datastore: None,
        network: None,
        node: None,
        ..profile(Provider::Aws)
    };
    let result = test_aws(&h, "").await;
    for c in &result.checks {
        println!("{:?} {}: {}", c.state, c.name, c.detail);
    }
    let launch = launch_check(&result);
    assert_eq!((launch.state, launch.detail.as_str()), (CheckState::Ok, "Able to launch EC2 instances."));
}

#[tokio::test]
async fn the_launch_probe_dry_runs_the_latest_amazon_linux_image_of_the_region() {
    let dry = fail("An error occurred (DryRunOperation) when calling the RunInstances operation: Request would have succeeded, but DryRun flag is set.");
    let (tools, result) = aws_checklist(ok("ami-07dcdf1066ae9dad3\n"), dry).await;
    assert!(result.ok, "{:?}", result.checks);
    assert_eq!((launch_check(&result).state, launch_check(&result).detail.as_str()), (CheckState::Ok, "Able to launch EC2 instances."));
    assert!(tools.called(&format!("ssm get-parameter --name {} --query Parameter.Value --output text", aws::LATEST_AMAZON_LINUX)), "{:?}", tools.calls());
    assert!(tools.called("ec2 run-instances --dry-run --instance-type t3.micro --image-id ami-07dcdf1066ae9dad3 --output text"), "{:?}", tools.calls());
    // The probe runs with the account's own keys and region, like every other call.
    assert_eq!(tools.env_of("ec2 run-instances", "AWS_REGION").as_deref(), Some("10.0.0.5"));
    assert_eq!(tools.env_of("ssm get-parameter", "AWS_SECRET_ACCESS_KEY").as_deref(), Some("s3cret"));
}

#[tokio::test]
async fn the_launch_probe_falls_back_to_an_unknown_image_when_the_parameter_cant_be_read() {
    let denied = fail(
        "An error occurred (AccessDeniedException) when calling the GetParameter operation: User: arn:aws:iam::1:user/me is not authorized to perform: ssm:GetParameter",
    );
    let not_found = fail("An error occurred (InvalidAMIID.NotFound) when calling the RunInstances operation: The image id '[ami-12345678]' does not exist");
    let (tools, result) = aws_checklist(denied, not_found).await;
    assert!(result.ok, "{:?}", result.checks);
    assert_eq!(launch_check(&result).state, CheckState::Ok);
    assert!(tools.called(&format!("--image-id {} ", aws::UNKNOWN_IMAGE)), "{:?}", tools.calls());
}

#[tokio::test]
async fn a_denied_dry_run_fails_the_checklist_with_the_identity() {
    let denied = fail("An error occurred (UnauthorizedOperation) when calling the RunInstances operation: You are not authorized to perform this operation.");
    let (_tools, result) = aws_checklist(ok("ami-07dcdf1066ae9dad3\n"), denied).await;
    assert!(!result.ok);
    let c = launch_check(&result);
    assert_eq!(c.state, CheckState::Fail);
    assert_eq!(c.detail, "arn:aws:iam::1:user/me can't launch EC2 (ec2:RunInstances is denied). Add EC2 permissions to it.");
}

#[tokio::test]
async fn a_throttled_dry_run_leaves_the_checklist_passing_with_a_warning() {
    let throttled = fail("An error occurred (RequestLimitExceeded) when calling the RunInstances operation: Request limit exceeded.");
    let (_tools, result) = aws_checklist(ok("ami-07dcdf1066ae9dad3\n"), throttled).await;
    assert_eq!(launch_check(&result).state, CheckState::Warn);
    assert!(launch_check(&result).detail.ends_with("Request limit exceeded."), "{}", launch_check(&result).detail);
}

#[test]
fn the_budget_line_compares_spend_with_the_limit() {
    let c = budget_outcome(50.0, Ok(12.345));
    assert_eq!((c.state, c.detail.as_str()), (CheckState::Ok, "$12.35 of $50 spent this month."));
    let c = budget_outcome(50.0, Ok(50.0));
    assert_eq!(c.state, CheckState::Fail);
    assert_eq!(c.detail, "Over budget: $50.00 of $50 this month. New labs here are blocked.");
    let c = budget_outcome(50.0, Err(aws_failed("Unable to locate credentials")));
    assert_eq!(c.state, CheckState::Warn);
    assert!(c.detail.ends_with("the AWS session has expired or isn't signed in."), "{}", c.detail);
}

#[test]
fn azure_sign_in_outcomes() {
    let (way, c) = first(azure_subscription(Ok("Pay-As-You-Go".into())));
    assert_eq!((way, c.detail.as_str()), ("pass", "Signed in to subscription \"Pay-As-You-Go\"."));
    let (way, c) = first(azure_subscription(Err(refused("az", azure::refusal, "ERROR: Please run 'az login' to setup account."))));
    assert_eq!(way, "refused");
    assert!(c.detail.starts_with("Not signed in to Azure"), "{}", c.detail);
    let (way, c) = first(azure_subscription(Err(missing())));
    assert_eq!((way, c.name.as_str()), ("unreachable", "Azure CLI"));
}

#[tokio::test]
async fn the_gcp_test_asks_for_the_sign_in_a_launch_needs_first() {
    // Regression: Test passed on the CLI's own sign-in while every launch was refused because
    // the application-default sign-in Terraform uses had run out.
    let tools = Tools::strict_rules(&[
        ("auth application-default print-access-token", fail("ERROR: (gcloud.auth.application-default.print-access-token) Reauthentication failed.")),
        ("billing accounts describe", ok("Main\n")),
        ("terraform version", ok("Terraform v1.9")),
    ]);
    let result = test_gcp(&profile(Provider::Gcp)).await;
    assert!(!result.ok);
    assert_eq!(result.checks.iter().map(|c| (c.name.as_str(), c.state)).collect::<Vec<_>>(), [("Sign-in", CheckState::Fail)]);
    assert!(result.checks[0].detail.contains("Reauthentication failed."), "{}", result.checks[0].detail);
    assert!(!tools.called("billing"), "{:?}", tools.calls());
    let _tools = Tools::strict_rules(&[
        ("auth application-default print-access-token", ok("ya29.token\n")),
        ("billing accounts describe", ok("Main\n")),
        ("terraform version", ok("Terraform v1.9")),
    ]);
    let result = test_gcp(&profile(Provider::Gcp)).await;
    assert!(result.ok, "{:?}", result.checks);
    assert_eq!(result.checks.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Sign-in", "Billing account", "Terraform"]);
}

#[test]
fn gcp_sign_in_outcomes() {
    let (way, c) = first(gcp_billing(Ok("My billing".into())));
    assert_eq!((way, c.detail.as_str()), ("pass", "Signed in; billing account \"My billing\"."));
    let (way, c) = first(gcp_billing(Err(refused("gcloud", gcp::refusal, "ERROR: (gcloud) PERMISSION_DENIED"))));
    assert_eq!(way, "refused");
    assert!(c.detail.starts_with("Not signed in to Google Cloud"), "{}", c.detail);
    let (way, c) = first(gcp_billing(Err(missing())));
    assert_eq!((way, c.name.as_str()), ("unreachable", "gcloud CLI"));
}

#[test]
fn token_api_outcomes() {
    use isothrum_machine::cloud::TokenError;
    let (way, c) = first(token_outcome("Linode", "Linodes", Ok(())));
    assert_eq!((way, c.detail.as_str()), ("pass", "Token is valid. Labs run as Linodes in this account."));
    let (way, c) = first(token_outcome("Linode", "Linodes", Err(TokenError::Rejected)));
    assert_eq!((way, c.detail.as_str()), ("refused", "Linode rejected this token. Create a new one with read/write scope."));
    let (way, c) = first(token_outcome("DigitalOcean", "droplets", Err(TokenError::Status(500))));
    assert_eq!((way, c.detail.as_str()), ("refused", "DigitalOcean API returned 500 Internal Server Error."));
    let (way, c) = first(token_outcome("DigitalOcean", "droplets", Err(TokenError::Unreachable("dns error".into()))));
    assert_eq!((way, c.detail.as_str()), ("unreachable", "Can't reach DigitalOcean: dns error"));
}

#[test]
fn oci_outcomes_and_a_missing_cli_only_warns() {
    let (way, c) = first(oci_compartment(Ok("root".into())));
    assert_eq!((way, c.state), ("pass", CheckState::Ok));
    assert_eq!(c.detail, "Reached compartment \"root\". Labs run in your tenancy.");
    let (way, c) = first(oci_compartment(Err(missing())));
    assert_eq!((way, c.state), ("pass", CheckState::Warn));
    let (way, c) = first(oci_compartment(Err(refused("oci", oci::refusal, "ServiceError: NotAuthenticated"))));
    assert_eq!(way, "refused");
    assert!(c.detail.starts_with("OCI rejected the request."), "{}", c.detail);
    let (way, _) = first(oci_compartment(Err(Failure::NotStarted { program: "oci".into(), reason: "boom".into() })));
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
