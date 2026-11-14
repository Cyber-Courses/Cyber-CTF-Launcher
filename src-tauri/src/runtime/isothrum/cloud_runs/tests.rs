use super::*;
use crate::platform::exec::fake::{Tools, fail, ok};

/// An AWS sign-in that ran out stops a change before anything runs, said as signing in again;
/// a valid one, or another cloud, lets it go on.
#[tokio::test]
async fn a_change_on_aws_waits_for_a_signed_in_account() {
    let home = std::env::temp_dir().join(format!("cyberctf-signin-{}", rand::random::<u32>()));
    let place = CloudPlace { region: Some("eu-west-3".into()), tf_env: vec![("AWS_PROFILE".into(), "labs".into())], ..CloudPlace::default() };
    let aws = CloudRun::new(&home, &home.join("lab"), "t", Runtime::Docker, "aws").unwrap();
    let tools = Tools::strict_rules(&[("sts get-caller-identity", fail("Error: login session has expired, please reauthenticate"))]);
    let err = aws.down(&place, &mut |_| ()).await;
    assert!(err.is_ok(), "no files: nothing to remove, nothing asked");
    let err = aws.update(&place, &mut |_| ()).await.unwrap_err().to_string();
    assert!(err.starts_with("Your sign-in to AWS has expired") && err.contains("`aws login`"), "{err}");
    assert_eq!(tools.calls().len(), 1, "only the question ran: {:?}", tools.calls());
    drop(tools);
    let signed = Tools::strict_rules(&[("sts get-caller-identity", ok("arn:aws:iam::1:user/me\n"))]);
    assert!(aws.signed_in(&place).await.is_ok());
    drop(signed);
    // Every cloud whose sign-in runs out is asked its own question; a token cloud, none.
    for (cloud, question, said) in [
        ("azure", "account get-access-token", "ERROR: AADSTS700082: The refresh token has expired. Please run 'az login'"),
        ("gcp", "application-default print-access-token", "Reauthentication failed. cannot prompt during non-interactive execution."),
        ("oci", "iam region list", "ServiceError: NotAuthenticated"),
    ] {
        let run = CloudRun::new(&home, &home.join("lab"), "t", Runtime::Docker, cloud).unwrap();
        let tools = Tools::strict_rules(&[(question, fail(said))]);
        let err = run.update(&place, &mut |_| ()).await.unwrap_err().to_string();
        assert!(err.starts_with("Your sign-in to") && tools.calls().len() == 1, "{cloud}: {err} {:?}", tools.calls());
    }
    let linode = CloudRun::new(&home, &home.join("lab"), "t", Runtime::Docker, "linode").unwrap();
    let none = Tools::strict_rules(&[]);
    assert!(linode.signed_in(&place).await.is_ok());
    assert!(none.calls().is_empty(), "no question for a token cloud");
    let _ = std::fs::remove_dir_all(&home);
}

/// A cloud run the runner lists as expired, as it records one: `cyberctf-<lab>`, started on
/// account `host`, its stop time long past. Its module is not there (the lab's files gone).
fn expired_record(home: &Path, lab: &str, host: &str) {
    let name = format!("cyberctf-{lab}");

    let record = serde_json::json!({
        "version": 0, "name": name, "project": home.join(lab),
        "format": "terraform", "cloud": "aws", "host_label": host, "state": "ready", "expires_at": 1
    });
    crate::testing::runner::save_run(home, &record);
}

/// A fresh runner home with each `(lab, host)` as an expired cloud run (`cyberctf-<lab>`).
fn home_with(records: &[(&str, &str)]) -> std::path::PathBuf {
    let home = std::env::temp_dir().join(format!("cyberctf-reap-{}", rand::random::<u32>()));
    for (lab, host) in records {
        expired_record(&home, lab, host);
    }
    home
}

/// An expired run whose account was removed since is left for a later sweep (the account may
/// come back), neither destroyed nor reported, and the sweep goes on: the other expired run is
/// still reaped (here its destroy fails, its lab's files being gone, so it is the one reported).
#[tokio::test]
async fn an_expired_run_whose_account_is_gone_is_skipped_and_the_others_still_swept() {
    let home = home_with(&[("orphan", "gone1"), ("web", "aws1")]);
    let asked = std::sync::Mutex::new(Vec::new());
    let account = |host: &str| {
        asked.lock().unwrap().push(host.to_string());
        (host == "aws1").then(|| vec![("AWS_ACCESS_KEY_ID".to_string(), "AKIA".to_string())])
    };
    crate::testing::logs::take_warnings();
    let failed = reap_expired(&home, SystemTime::now(), account, |_| Some(())).await.unwrap();
    // Its lab isn't installed: its files can't be generated again, which is said.
    let warned = crate::testing::logs::take_warnings();
    assert!(warned.iter().any(|w| w.starts_with("auto-stop: cyberctf-web's files can't be generated again (lab `web` is not installed)")), "{warned:?}");
    assert_eq!(asked.into_inner().unwrap(), ["gone1", "aws1"], "every expired run's account is looked up");
    let names: Vec<&str> = failed.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["cyberctf-web"], "{failed:?}");
    assert!(crate::testing::runner::is_recorded(&home, "cyberctf-orphan"), "the skipped run keeps its record");
    std::fs::remove_dir_all(home).unwrap();
}

/// A lab with an operation in flight (its per-lab lock held) is left for a later sweep: the
/// reaper serializes with a user Stop/install, so it never runs a destroy over the same
/// Terraform state. The busy lab's account is not even looked up and it is not reported a
/// failure; the other expired run is still swept.
#[tokio::test]
async fn a_run_whose_lab_is_busy_is_left_for_a_later_sweep() {
    let home = home_with(&[("busy", "aws1"), ("web", "aws1")]);
    let asked = std::sync::Mutex::new(Vec::new());
    let account = |host: &str| {
        asked.lock().unwrap().push(host.to_string());
        Some(vec![("AWS_ACCESS_KEY_ID".to_string(), "AKIA".to_string())])
    };
    // "busy"'s lock can't be taken (another operation holds it); "web"'s can.
    let lock = |id: &str| (id != "busy").then_some(());
    let failed = reap_expired(&home, SystemTime::now(), account, lock).await.unwrap();
    assert_eq!(asked.into_inner().unwrap(), ["aws1"], "only the free lab's account is used, never the busy one's");
    let names: Vec<&str> = failed.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["cyberctf-web"], "the free lab is swept (its files are gone, so its destroy fails); the busy one is not touched: {failed:?}");
    assert!(crate::testing::runner::is_recorded(&home, "cyberctf-busy"), "the busy run keeps its record (never destroyed)");
    std::fs::remove_dir_all(home).unwrap();
}

/// An expired run with no account recorded can't be destroyed by the launcher: reported with
/// the code the lab page says in the player's language, which tells them to remove it by hand.
#[tokio::test]
async fn an_expired_run_with_no_account_is_reported_as_such() {
    let home = home_with(&[("web", "aws1")]);
    let mut record = crate::testing::runner::run_record(&home, "cyberctf-web");
    record.as_object_mut().unwrap().remove("host_label");
    crate::testing::runner::save_run(&home, &record);
    let failed = reap_expired(&home, SystemTime::now(), |_: &str| None, |_| Some(())).await.unwrap();
    let codes: Vec<(&str, Code)> = failed.iter().map(|(name, e)| (name.as_str(), e.code())).collect();
    assert_eq!(codes, [("cyberctf-web", Code::ReapNoHost)]);
    std::fs::remove_dir_all(home).unwrap();
}

fn place() -> CloudPlace {
    CloudPlace {
        label: "aws1".into(),
        region: Some("us-east-1".into()),
        allowed_cidr: Some("203.0.113.7/32".into()),
        tf_env: [("AWS_ACCESS_KEY_ID", "AKIA"), ("AWS_SECRET_ACCESS_KEY", "sk"), ("AWS_PROFILE", "labs"), ("AWS_REGION", "us-east-1")]
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        lifetime: Some(Duration::from_secs(4 * 3600)),
        project: None,
    }
}

#[test]
fn a_module_of_the_labs_own_gets_where_the_account_works() {
    // Regression: the cloud-services target's AWS_REGION (and Azure's subscription, Google's
    // project and region) was never given, so a module whose provider names no region had none.
    let aws = place().options(&[]);
    assert_eq!(aws.env.get("AWS_REGION").map(String::as_str), Some("us-east-1"));
    let azure = CloudPlace { tf_env: vec![("ARM_SUBSCRIPTION_ID".into(), "sub-1".into())], ..CloudPlace::default() };
    assert_eq!(azure.options(&[]).env.get("ARM_SUBSCRIPTION_ID").map(String::as_str), Some("sub-1"));
    let gcp = CloudPlace { region: Some("europe-west1".into()), project: Some("cyberctf-labs-1".into()), ..CloudPlace::default() };
    let gcp = gcp.options(&[]);
    assert_eq!(gcp.env.get("GOOGLE_PROJECT").map(String::as_str), Some("cyberctf-labs-1"));
    assert_eq!(gcp.env.get("GOOGLE_REGION").map(String::as_str), Some("europe-west1"));
}

#[test]
fn credentials_are_connection_secrets_while_region_and_profile_are_parameters() {
    let opts = place().options(&[("FLAG".to_owned(), "flag{x}".to_owned())]);
    // The account's credentials and the launch's input slots travel as secrets…
    assert_eq!(opts.secrets.get("AWS_ACCESS_KEY_ID").map(SecretValue::expose), Some("AKIA"));
    assert_eq!(opts.secrets.get("AWS_SECRET_ACCESS_KEY").map(SecretValue::expose), Some("sk"));
    assert_eq!(opts.secrets.get("FLAG").map(SecretValue::expose), Some("flag{x}"));
    // …the region and profile do not (they are plain values).
    assert!(opts.secrets.get("AWS_PROFILE").is_none() && opts.secrets.get("AWS_REGION").is_none());
    assert_eq!(opts.env.get("region").map(String::as_str), Some("us-east-1"));
    assert_eq!(opts.env.get("allowed_cidr").map(String::as_str), Some("203.0.113.7/32"));
    assert_eq!(opts.env.get("AWS_PROFILE").map(String::as_str), Some("labs"));
    assert_eq!(opts.host_label.as_deref(), Some("aws1"));
    assert!(opts.lifetime.is_some(), "the account's auto-stop is the run's expiry");
}

#[test]
fn a_destroy_takes_the_credentials_as_secrets_and_the_region_and_profile_as_values() {
    let tf_env: Vec<(String, String)> = [
        ("AWS_ACCESS_KEY_ID", "AKIA"),
        ("AWS_SECRET_ACCESS_KEY", "sk"),
        ("AWS_PROFILE", "labs"),
        ("AWS_REGION", "us-east-1"),
        ("AWS_DEFAULT_REGION", "us-east-1"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let opts = destroy_options(&tf_env);
    assert_eq!(opts.secrets.get("AWS_ACCESS_KEY_ID").map(SecretValue::expose), Some("AKIA"));
    assert_eq!(opts.secrets.get("AWS_SECRET_ACCESS_KEY").map(SecretValue::expose), Some("sk"));
    assert!(opts.secrets.get("AWS_PROFILE").is_none(), "the profile is a value, not a credential");
    assert!(opts.secrets.get("AWS_REGION").is_none() && opts.secrets.get("AWS_DEFAULT_REGION").is_none(), "the region is a value");
    assert_eq!(opts.env.get("AWS_REGION").map(String::as_str), Some("us-east-1"));
}

#[test]
fn an_operation_without_a_launch_sets_no_firewall_or_expiry() {
    let place = CloudPlace { region: Some("eu-west-3".to_owned()), tf_env: vec![("AWS_ACCESS_KEY_ID".to_owned(), "k".to_owned())], ..Default::default() };
    let opts = place.options(&[]);
    assert!(!opts.env.contains_key("allowed_cidr"), "no firewall value at destroy");
    assert!(opts.lifetime.is_none());
    assert_eq!(opts.env.get("region").map(String::as_str), Some("eu-west-3"));
    assert_eq!(opts.secrets.get("AWS_ACCESS_KEY_ID").map(SecretValue::expose), Some("k"));
}
