use super::*;
use crate::platform::exec::fake::{Tools, fail, ok};

fn sink() -> (impl FnMut(String), std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let s = seen.clone();
    (move |line| s.lock().unwrap().push(line), seen)
}

#[tokio::test]
async fn the_cloud_commands_read_their_clis() {
    let tools = Tools::new(|program, args| match (program, args) {
        ("aws", ["sts", ..]) => ok("arn:aws:iam::123456789012:user/ada\n"),
        ("aws", ["configure", "list-profiles"]) => ok("default\nlabs\n\n"),
        ("aws", ["ce", ..]) => ok("12.5\n"),
        ("az", ["account", "list", ..]) => ok(r#"[{"name":"Pay-As-You-Go","id":"00000000-0000-0000-0000-000000000000","isDefault":true}]"#),
        ("gcloud", ["auth", "list", ..]) => ok("ada@example.com\n"),
        ("gcloud", ["billing", ..]) => ok(r#"[{"name":"billingAccounts/AAAAAA-BBBBBB-CCCCCC","displayName":"Main","open":true}]"#),
        ("gcloud", ["organizations", ..]) => ok(r#"[{"name":"organizations/123","displayName":"Org"}]"#),
        ("aws" | "az" | "gcloud", [..]) => ok("signed in\n"),
        _ => None,
    });
    assert!(aws_cli_identity(Some("labs".into())).await.is_some());
    assert!(tools.called("--profile labs"));
    assert_eq!(aws_profiles().await, ["default", "labs"]);
    assert_eq!(aws_month_to_date_cost(None).await, Some(12.5));
    assert_eq!(azure_subscriptions().await.len(), 1);
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
    // Nothing to cancel any more: a no-op.
    cloud_login_cancel();
}

#[tokio::test]
async fn an_expired_application_default_sign_in_reads_as_signed_out() {
    // Regression: the setup asked only for the CLI's own token and said "signed in as …",
    // while every launch asks for the application-default one Terraform uses, and was
    // refused with "sign in again".
    let tools = Tools::new(|_, args| match args {
        ["auth", "list", ..] => ok("ada@example.com\n"),
        ["auth", "print-access-token", ..] => ok("ya29.token\n"),
        ["auth", "application-default", "print-access-token"] => fail("Reauthentication failed. cannot prompt during non-interactive execution."),
        _ => None,
    });
    crate::testing::logs::take_warnings();
    assert_eq!(gcp_account().await, None);
    assert!(tools.called("gcloud auth application-default print-access-token"), "{:?}", tools.calls());
    // Said in the log, as its siblings' failures are: it dropped them with `.ok()?`.
    let said = crate::testing::logs::take_warnings();
    assert!(said.iter().any(|w| w.contains("Google Cloud's application-default sign-in") && w.contains("Reauthentication failed")), "{said:?}");
}

#[tokio::test]
async fn signed_out_clis_read_as_nothing() {
    let _tools = Tools::new(|_, args| match args {
        ["auth", "list", ..] => ok("ada@example.com\n"),
        _ => fail("expired"),
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
    let _tools = Tools::new(|program, args| match (program, args) {
        ("gcloud", ["billing", ..]) => fail("Cloud Billing API has not been used in project 12 before or it is disabled."),
        ("gcloud", ["organizations", ..]) => ok("Listed 0 items."),
        ("az", ..) => None,
        _ => fail("AccessDenied"),
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
    assert_eq!(login_command(&Cloud::Azure), ("az", &["login"][..]));
    // gcloud's login also writes the credentials Terraform reads.
    assert_eq!(login_command(&Cloud::Gcp), ("gcloud", &["auth", "login", "--update-adc"][..]));
}

#[test]
fn a_profile_is_passed_only_when_chosen() {
    assert_eq!(with_profile(vec!["login"], None), ["login"]);
    assert_eq!(with_profile(vec!["login"], Some("work")), ["login", "--profile", "work"]);
}

#[tokio::test]
async fn a_profiles_spend_is_read_with_that_profile() {
    let tools = Tools::expect(&[("aws ce get-cost-and-usage --time-period", ok("12.5\n"))]);
    assert_eq!(aws_month_to_date_cost(Some("work".into())).await, Some(12.5));
    assert_eq!(tools.env_of("ce get-cost-and-usage", "AWS_PROFILE").as_deref(), Some("work"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_sign_in_can_be_cancelled_and_never_waits_forever() {
    // Regression: `cloud_login` had no time limit and no cancel: a CLI waiting for a browser
    // sign-in nobody finished kept the setup busy for good.
    // Its own cancel, so tests running at the same time can't end it.
    let (cancel, cancelled) = oneshot::channel();
    let waiting = tokio::spawn(sign_in_within(Duration::from_secs(30), crate::platform::exec::stream("sh", &["-c", "sleep 30"], None, &[], |_| {}), cancelled));
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
fn cli_answers_are_trimmed_lines() {
    assert_eq!(non_empty(" arn:aws:iam::1:user/a \n").as_deref(), Some("arn:aws:iam::1:user/a"));
    assert_eq!(non_empty("  \n"), None);
    assert_eq!(lines("default\n  work \n\n"), ["default", "work"]);
    assert_eq!(lines(""), [] as [&str; 0]);
}

#[test]
fn azure_subscriptions_parse_or_are_none() {
    let subs = subscriptions(r#"[{"name": "Pay-As-You-Go", "id": "0000", "isDefault": true}, {"name": "Dev", "id": "1111"}]"#);
    assert_eq!(subs.len(), 2);
    assert_eq!((subs[0].name.as_str(), subs[0].id.as_str(), subs[0].is_default), ("Pay-As-You-Go", "0000", true));
    assert!(!subs[1].is_default);
    assert!(subscriptions("Please run 'az login'").is_empty());
}

#[test]
fn gcp_lists_drop_prefixes_and_nameless_entries() {
    let accounts = billing_accounts(
        r#"[{"name": "billingAccounts/0X0X0X-0X0X0X-0X0X0X", "displayName": "Main", "open": true}, {"displayName": "No id"}, {"name": "billingAccounts/AAAAAA-BBBBBB-CCCCCC"}]"#,
    );
    assert_eq!(accounts.len(), 2);
    assert_eq!((accounts[0].id.as_str(), accounts[0].name.as_str(), accounts[0].open), ("0X0X0X-0X0X0X-0X0X0X", "Main", true));
    assert!(!accounts[1].open && accounts[1].name.is_empty());
    let orgs = organizations(r#"[{"name": "organizations/1234567890", "displayName": "example.com"}, {}]"#);
    assert_eq!(orgs.len(), 1);
    assert_eq!((orgs[0].id.as_str(), orgs[0].name.as_str()), ("1234567890", "example.com"));
    assert!(billing_accounts("not json").is_empty());
    assert!(organizations("").is_empty());
}

#[tokio::test]
async fn the_oci_config_command_reads_its_file_or_reports_none() {
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
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_oci_config_is_read_from_its_default_profile() {
    let text = "[OTHER]\ntenancy=ocid1.tenancy.oc1..other\n\n[DEFAULT]\nuser=ocid1.user.oc1..u\n tenancy = ocid1.tenancy.oc1..main \nregion=eu-frankfurt-1\nnot a pair\n[LATER]\nregion=us-ashburn-1\n";
    let c = parse_oci_config(text);
    assert!(c.configured);
    assert_eq!(c.tenancy, "ocid1.tenancy.oc1..main");
    assert_eq!(c.region, "eu-frankfurt-1");
    // The profile name is matched case-insensitively.
    assert_eq!(parse_oci_config("[default]\nregion=x\n").region, "x");
    // A file without a DEFAULT profile is still a config, with nothing to prefill.
    let c = parse_oci_config("[PROD]\nregion=x\n");
    assert!(c.configured && c.region.is_empty() && c.tenancy.is_empty());
}

#[test]
fn cli_json_that_is_not_a_list_of_objects_reads_as_nothing() {
    // An object, null, nothing, a warning printed before the JSON, and list entries that
    // aren't objects: none of them is a subscription or an account, and none panics.
    for out in ["{}", "null", "", "WARNING: update available\n[]", "[1, \"x\", null]"] {
        assert!(subscriptions(out).is_empty(), "{out:?}");
        assert!(billing_accounts(out).is_empty(), "{out:?}");
        assert!(organizations(out).is_empty(), "{out:?}");
    }
    // Fields of the wrong type fall back rather than dropping the entry.
    let accounts = billing_accounts(r#"[{"name": "billingAccounts/A", "displayName": 7, "open": "true"}]"#);
    assert_eq!((accounts[0].id.as_str(), accounts[0].name.as_str(), accounts[0].open), ("A", "", false));
    // An id that is only the prefix is empty, so it is dropped.
    assert!(billing_accounts(r#"[{"name": "billingAccounts/"}]"#).is_empty());
}

#[tokio::test]
async fn a_profile_reaches_the_cli_as_one_argument_however_it_is_spelled() {
    // No shell in between: a profile with a space is one argument, not two.
    let tools = Tools::new(|_, args| match args {
        [.., "--profile", _] => ok("arn:aws:iam::1:user/a\n"),
        _ => fail("Unable to locate credentials"),
    });
    assert!(aws_cli_identity(Some("my profile".into())).await.is_some());
    assert!(tools.calls().last().unwrap().ends_with(" --profile my profile"), "{:?}", tools.calls());
    assert_eq!(aws_cli_identity(None).await, None, "no profile: none passed");
    assert!(!tools.calls().last().unwrap().contains("--profile"));
}

#[test]
fn oci_config_lines_at_the_edges() {
    // CRLF endings, a later DEFAULT section overriding, a value holding `=`, a key with
    // spaces around it, and an empty value.
    let text = "[DEFAULT]\r\nregion=eu-paris-1\r\ntenancy = ocid1.tenancy.oc1..a=b \r\n[OTHER]\r\nregion=x\r\n[DEFAULT]\r\nregion=\r\n";
    let c = parse_oci_config(text);
    assert_eq!((c.tenancy.as_str(), c.region.as_str()), ("ocid1.tenancy.oc1..a=b", ""));
    // A section header with trailing text is another section.
    assert_eq!(parse_oci_config("[DEFAULT] ; main\nregion=x\n").region, "");
    assert!(parse_oci_config("").configured && parse_oci_config("").region.is_empty());
}
