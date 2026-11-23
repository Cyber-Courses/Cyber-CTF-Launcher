use super::*;
use isothrum_runner::fake::{FakeProcesses, Reply};

use crate::runtime::providers::testing::{Faked, fake};

/// The CLIs answered by `answer`, given each call's program and arguments.
fn answering(answer: impl Fn(&str, &[&str]) -> Reply + Send + Sync + 'static) -> Faked {
    fake(FakeProcesses::model(move |call| answer(&call.program, &call.args.iter().map(String::as_str).collect::<Vec<_>>())))
}

fn sink() -> (impl FnMut(String), std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let s = seen.clone();
    (move |line| s.lock().unwrap().push(line), seen)
}

#[tokio::test]
async fn the_cloud_commands_read_their_clis() {
    let tools = answering(|program, args| match (program, args) {
        ("aws", ["sts", ..]) => Reply::ok("arn:aws:iam::123456789012:user/ada\n"),
        ("aws", ["configure", "list-profiles"]) => Reply::ok("default\nlabs\n\n"),
        ("aws", ["ce", ..]) => Reply::ok("12.5\n"),
        ("az", ["account", "list", ..]) => Reply::ok(r#"[{"name":"Pay-As-You-Go","id":"00000000-0000-0000-0000-000000000000","isDefault":true}]"#),
        ("gcloud", ["auth", "list", ..]) => Reply::ok("ada@example.com\n"),
        ("gcloud", ["billing", ..]) => Reply::ok(r#"[{"name":"billingAccounts/AAAAAA-BBBBBB-CCCCCC","displayName":"Main","open":true}]"#),
        ("gcloud", ["organizations", ..]) => Reply::ok(r#"[{"name":"organizations/123","displayName":"Org"}]"#),
        ("aws" | "az" | "gcloud", [..]) => Reply::ok("signed in\n"),
        _ => Reply::missing(),
    });
    assert_eq!(aws_cli_identity(Some("labs".into())).await.as_deref(), Some("arn:aws:iam::123456789012:user/ada"));
    assert_eq!(tools.last("sts get-caller-identity").as_ref().and_then(|c| c.env("AWS_PROFILE")), Some("labs"));
    assert_eq!(aws_profiles().await, ["default", "labs"]);
    assert_eq!(aws_month_to_date_cost(None).await, Some(12.5));
    let subs = azure_subscriptions().await;
    assert_eq!((subs.len(), subs[0].name.as_str(), subs[0].is_default), (1, "Pay-As-You-Go", true));
    assert_eq!(gcp_account().await.as_deref(), Some("ada@example.com"));
    let billing = gcp_billing_accounts().await;
    assert_eq!((billing.len(), billing[0].id.as_str(), billing[0].name.as_str(), billing[0].open), (1, "AAAAAA-BBBBBB-CCCCCC", "Main", true));
    let orgs = gcp_organizations().await;
    assert_eq!((orgs.len(), orgs[0].id.as_str(), orgs[0].name.as_str()), (1, "123", "Org"));
    let (channel, seen) = sink();
    cloud_login(Cloud::Azure, channel).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), ["$ az login", "signed in"]);
    let (channel, seen) = sink();
    aws_login(Some("labs".into()), channel).await.unwrap();
    assert_eq!(seen.lock().unwrap()[0], "$ aws login --profile labs");
    let (channel, seen) = sink();
    cloud_login(Cloud::Gcp, channel).await.unwrap();
    // gcloud's login also writes the credentials Terraform reads.
    assert_eq!(seen.lock().unwrap()[0], "$ gcloud auth login --update-adc");
    // Nothing to cancel any more: a no-op.
    cloud_login_cancel();
}

#[tokio::test]
async fn an_expired_application_default_sign_in_reads_as_signed_out() {
    // Regression: the setup asked only for the CLI's own token and said "signed in as …",
    // while every launch asks for the application-default one Terraform uses, and was
    // refused with "sign in again".
    let tools = answering(|_, args| match args {
        ["auth", "list", ..] => Reply::ok("ada@example.com\n"),
        ["auth", "print-access-token", ..] => Reply::ok("ya29.token\n"),
        ["auth", "application-default", "print-access-token"] => Reply::fail(1, "Reauthentication failed. cannot prompt during non-interactive execution."),
        _ => Reply::missing(),
    });
    crate::testing::logs::take_warnings();
    assert_eq!(gcp_account().await, None);
    assert!(tools.last("gcloud auth application-default print-access-token").is_some(), "{:?}", tools.lines());
    // Said in the log, as its siblings' failures are: it dropped them with `.Reply::ok()?`.
    let said = crate::testing::logs::take_warnings();
    assert!(said.iter().any(|w| w.contains("Google Cloud's application-default sign-in") && w.contains("Reauthentication failed")), "{said:?}");
}

#[tokio::test]
async fn signed_out_clis_read_as_nothing() {
    let _tools = answering(|_, args| match args {
        ["auth", "list", ..] => Reply::ok("ada@example.com\n"),
        _ => Reply::fail(1, "expired"),
    });
    assert_eq!(aws_cli_identity(None).await, None);
    assert_eq!(aws_profiles().await, [] as [&str; 0]);
    assert_eq!(aws_month_to_date_cost(None).await, None);
    assert!(azure_subscriptions().await.is_empty());
    assert_eq!(gcp_account().await, None, "an expired sign-in counts as signed out");
    assert!(gcp_billing_accounts().await.is_empty() && gcp_organizations().await.is_empty());
    assert!(cloud_login(Cloud::Gcp, sink().0).await.is_err());
}

/// Regression: every list read a failure as "none", nothing logged, so a setup showing no
/// billing account (the Cloud Billing API off, a permission denied) couldn't be explained.
#[tokio::test]
async fn a_cli_list_that_fails_or_doesnt_parse_is_logged_and_a_missing_cli_is_not() {
    let _tools = answering(|program, args| match (program, args) {
        ("gcloud", ["billing", ..]) => Reply::fail(1, "Cloud Billing API has not been used in project 12 before or it is disabled."),
        ("gcloud", ["organizations", ..]) => Reply::ok("Listed 0 items."),
        ("az", ..) => Reply::missing(),
        _ => Reply::fail(1, "AccessDenied"),
    });
    crate::testing::logs::take_warnings();
    assert!(gcp_billing_accounts().await.is_empty());
    let warned = crate::testing::logs::take_warnings();
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(
        warned[0].starts_with("couldn't read the Google Cloud billing accounts, so none is offered: ") && warned[0].contains("Cloud Billing API"),
        "{warned:?}"
    );
    assert!(gcp_organizations().await.is_empty());
    let warned = crate::testing::logs::take_warnings();
    assert!(warned.len() == 1 && warned[0].starts_with("couldn't read the Google Cloud organizations from the CLI's answer"), "{warned:?}");
    assert_eq!(aws_month_to_date_cost(None).await, None);
    assert_eq!(aws_profiles().await, [] as [&str; 0]);
    assert_eq!(aws_cli_identity(None).await, None);
    let warned = crate::testing::logs::take_warnings();
    assert_eq!(warned.len(), 3, "the spend, the profiles and the identity: {warned:?}");
    assert!(warned[2].starts_with("couldn't read the AWS CLI's identity"), "{warned:?}");
    assert!(azure_subscriptions().await.is_empty());
    assert_eq!(crate::testing::logs::take_warnings(), [] as [String; 0], "a CLI that isn't installed is no failure");
}

#[test]
fn each_cloud_signs_in_with_its_own_cli() {
    // AWS isn't one: `aws_login` signs in with `aws login` (the old `aws sso login` path was
    // never called).
    assert!(serde_json::from_str::<Cloud>(r#""aws""#).is_err());
    assert!(serde_json::from_str::<Cloud>(r#""azure""#).is_ok() && serde_json::from_str::<Cloud>(r#""gcp""#).is_ok());
}

#[tokio::test]
async fn a_profiles_spend_is_read_with_that_profile() {
    let tools = fake(FakeProcesses::script().expect("aws ce get-cost-and-usage --time-period", Reply::ok("12.5\n")).build());
    assert_eq!(aws_month_to_date_cost(Some("work".into())).await, Some(12.5));
    assert_eq!(tools.last("ce get-cost-and-usage").as_ref().and_then(|c| c.env("AWS_PROFILE")), Some("work"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_sign_in_can_be_cancelled_and_never_waits_forever() {
    // Regression: `cloud_login` had no time limit and no cancel: a CLI waiting for a browser
    // sign-in nobody finished kept the setup busy for good.
    // Its own cancel, so tests running at the same time can't end it.
    let (cancel, cancelled) = oneshot::channel();
    let waiting = tokio::spawn(sign_in_within(Duration::from_secs(30), std::future::pending(), cancelled));
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.send(()).unwrap();
    let err = tokio::time::timeout(Duration::from_secs(5), waiting).await.expect("ended at once").unwrap().unwrap_err();
    assert_eq!(err.code(), Code::SignInCancelled);
    let (_keep, never) = oneshot::channel();
    let err = sign_in_within(Duration::from_millis(50), std::future::pending(), never).await.unwrap_err();
    assert_eq!(err.code(), Code::LoginTimeout { minutes: 0 });
    assert_eq!(LOGIN_TIMEOUT, Duration::from_secs(600));
    // The command cancels the sign-in in progress.
    let pending = cancellable();
    cloud_login_cancel();
    // (A sign-in started meanwhile by another test cancels it too.)
    assert!(pending.await.is_ok());
}

#[test]
fn a_cli_failure_reads_as_the_launchers_error_a_missing_cli_as_missing_and_a_stuck_one_as_timed_out() {
    let missing = tool_error(&Failure::Missing { program: "gcloud".into() });
    assert!(matches!(missing, Error::ToolMissing { tool: "gcloud" }), "{missing:?}");
    let stuck = tool_error(&Failure::TimedOut { program: "az".into(), limit: Duration::from_secs(60) });
    assert!(crate::platform::exec::timed_out(&stuck), "{stuck:?}");
    assert!(stuck.to_string().contains("timed out after 60s"), "{stuck}");
    let refused = tool_error(&Failure::Refused { program: "aws".into(), refusal: isothrum_machine::Refusal::Other, said: "Rate exceeded".into() });
    assert!(matches!(&refused, Error::CommandFailed { stderr, .. } if stderr == "Rate exceeded"), "{refused:?}");
}

#[test]
fn the_oci_config_command_reads_its_file_or_reports_none() {
    // The command reads the file the CLI would (tests don't touch the environment).
    assert_eq!(oci_config(), read_oci_config(oci_config_path().as_deref()));
    assert!(oci_config_path().is_some_and(|p| p.ends_with("config")));
    let dir = std::env::temp_dir().join(format!("cyberctf-oci-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("config");
    assert_eq!(read_oci_config(Some(&file)), OciConfig::default());
    assert_eq!(read_oci_config(None), OciConfig::default());
    std::fs::write(&file, "[DEFAULT]\ntenancy=ocid1.tenancy.oc1..t\nregion=eu-paris-1\n").unwrap();
    let read = read_oci_config(Some(&file));
    assert_eq!(read, OciConfig { configured: true, tenancy: "ocid1.tenancy.oc1..t".into(), region: "eu-paris-1".into() });
    // A file without a DEFAULT profile is still a config, with nothing to prefill.
    std::fs::write(&file, "[PROD]\nregion=x\n").unwrap();
    assert_eq!(read_oci_config(Some(&file)), OciConfig { configured: true, ..OciConfig::default() });
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_profile_reaches_the_cli_however_it_is_spelled() {
    // No shell in between: a profile with a space is one value, not two.
    let tools = answering(|_, args| match args {
        ["sts", ..] => Reply::ok("arn:aws:iam::1:user/a\n"),
        _ => Reply::fail(1, "Unable to locate credentials"),
    });
    assert!(aws_cli_identity(Some("my profile".into())).await.is_some());
    assert_eq!(tools.last("sts get-caller-identity").as_ref().and_then(|c| c.env("AWS_PROFILE")), Some("my profile"));
}
