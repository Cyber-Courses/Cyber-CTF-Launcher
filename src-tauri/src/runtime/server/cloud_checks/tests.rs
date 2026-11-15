use super::*;
use crate::platform::exec::fake::{Reply, Tools, fail, ok};
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
    let denied = Err(failed("An error occurred (UnauthorizedOperation) when calling the RunInstances operation: You are not authorized"));
    let c = launch_permission(&denied, "arn:x");
    assert_eq!(c.state, CheckState::Fail);
    assert!(c.detail.starts_with("arn:x can't launch EC2"), "{}", c.detail);
    // DryRunOperation is how AWS says "would have succeeded".
    let c =
        launch_permission(&Err(failed("An error occurred (DryRunOperation) when calling the RunInstances operation: Request would have succeeded")), "arn:x");
    assert_eq!((c.state, c.detail.as_str()), (CheckState::Ok, "Able to launch EC2 instances."));
}

#[test]
fn an_error_aws_raises_after_authorizing_the_dry_run_passes_the_permission_check() {
    // Regression: the probe used a made-up image id, which AWS refuses as malformed before it
    // looks at the permission, so every signed-in account read "Needs attention". The unknown
    // well-formed image (NotFound) and anything else past authorization mean the permission
    // itself passed.
    for stderr in [
        "An error occurred (InvalidAMIID.NotFound) when calling the RunInstances operation: The image id '[ami-12345678]' does not exist",
        "An error occurred (VPCIdNotSpecified) when calling the RunInstances operation: No default VPC for this user",
        "An error occurred (InstanceLimitExceeded) when calling the RunInstances operation: You have requested more vCPU capacity than your current limit",
    ] {
        let c = launch_permission(&Err(failed(stderr)), "arn:x");
        assert_eq!(c.state, CheckState::Ok, "{stderr}");
        assert!(c.detail.starts_with("Able to launch EC2 instances (ec2:RunInstances is allowed; the dry run then stopped at "), "{}", c.detail);
    }
    let c = launch_permission(&Err(failed("x\nAn error occurred (VPCIdNotSpecified) when calling the RunInstances operation: No default VPC")), "arn:x");
    assert!(c.detail.contains("stopped at VPCIdNotSpecified,"), "{}", c.detail);
}

#[test]
fn a_dry_run_that_failed_before_authorization_is_a_warning_with_its_reason_not_a_pass() {
    // Throttling, an expired token, a malformed id, AWS itself: none says whether the
    // permission is there.
    for stderr in [
        "An error occurred (RequestLimitExceeded) when calling the RunInstances operation",
        "An error occurred (ExpiredToken) when calling the RunInstances operation: The security token included in the request is expired",
        "An error occurred (InvalidAMIID.Malformed) when calling the RunInstances operation: Invalid id: \"ami-00000000000000000\" (expecting \"ami-...\")",
        "An error occurred (InvalidParameterValue) when calling the RunInstances operation: Value (t3.micro) for parameter instanceType is invalid",
        "An error occurred (InternalError) when calling the RunInstances operation: An internal error has occurred",
    ] {
        let c = launch_permission(&Err(failed(stderr)), "arn:x");
        assert_eq!(c.state, CheckState::Warn, "{stderr}");
        assert_eq!(c.detail, format!("Couldn't confirm EC2 launches are allowed: {stderr}"));
    }
    // No AWS error code at all: the CLI or the network, quoted.
    let c = launch_permission(&Err(failed("Could not connect to the endpoint URL: \"https://ec2.eu-west-3.amazonaws.com/\"")), "arn:x");
    assert_eq!(c.state, CheckState::Warn);
    assert!(c.detail.ends_with("Could not connect to the endpoint URL: \"https://ec2.eu-west-3.amazonaws.com/\""), "{}", c.detail);
    let timed_out = launch_permission(&Err(missing()), "arn:x");
    assert_eq!(timed_out.state, CheckState::Warn);
    assert_eq!(launch_permission(&Ok(String::new()), "arn:x").state, CheckState::Warn);
}

#[test]
fn the_probe_image_is_the_one_ssm_printed_else_the_unknown_well_formed_one() {
    assert_eq!(probe_image(Ok("ami-07dcdf1066ae9dad3\n".into())), "ami-07dcdf1066ae9dad3");
    assert_eq!(probe_image(Ok(String::new())), UNKNOWN_IMAGE);
    assert_eq!(probe_image(Ok("None\n".into())), UNKNOWN_IMAGE);
    assert_eq!(probe_image(Ok("ami-1 ami-2".into())), UNKNOWN_IMAGE);
    assert_eq!(probe_image(Err(failed("An error occurred (AccessDeniedException) when calling the GetParameter operation"))), UNKNOWN_IMAGE);
    assert_eq!(probe_image(Err(missing())), UNKNOWN_IMAGE);
    assert_eq!(aws_error_code("x\nAn error occurred (InvalidAMIID.NotFound) when calling"), Some("InvalidAMIID.NotFound"));
    assert_eq!(aws_error_code("An error occurred (broken"), None);
    assert_eq!(aws_error_code("timed out"), None);
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
    assert!(tools.called(&format!("ssm get-parameter --name {LATEST_AMAZON_LINUX} --query Parameter.Value --output text")), "{:?}", tools.calls());
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
    assert!(tools.called(&format!("--image-id {UNKNOWN_IMAGE} ")), "{:?}", tools.calls());
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
