use super::*;
use crate::runtime::Runtime;

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
fn a_destroy_takes_the_credentials_as_secrets_only() {
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
    assert!(opts.env.is_empty(), "the runner replays the start's values");
}

#[test]
fn a_later_operation_gives_the_credentials_and_leaves_the_values_to_the_runners_replay() {
    // The runner replays the start's region, firewall CIDR and profile: a stop given the
    // account's region now would act on another region than the start's.
    let place = CloudPlace {
        label: "aws1".to_owned(),
        region: Some("eu-west-3".to_owned()),
        tf_env: vec![("AWS_ACCESS_KEY_ID".to_owned(), "k".to_owned())],
        ..Default::default()
    };
    let opts = place.later_options();
    assert!(opts.env.is_empty(), "no value over the start's");
    assert!(opts.lifetime.is_none());
    assert_eq!(opts.secrets.get("AWS_ACCESS_KEY_ID").map(SecretValue::expose), Some("k"));
    assert_eq!(opts.host_label.as_deref(), Some("aws1"));
}

/// Isoloom's Terraform modules ask for the key their machines let in, with no default: a run
/// gives them the runner's own, its public key made when there is none, and its private key's
/// file, which the runner signs in with.
#[tokio::test]
async fn a_run_gives_its_module_the_runners_key() {
    let home = std::env::temp_dir().join(format!("cyberctf-keys-{}", rand::random::<u32>()));
    let run = Deployment::in_cloud(&home, &home.join("lab"), "t", Runtime::Vm, "aws", CloudPlace::default()).unwrap();
    let values = run.key_values().await.unwrap();
    let value = |name: &str| values.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default();
    assert!(value("ssh_public_key").starts_with("ssh-ed25519 "), "{values:?}");
    let private = std::path::PathBuf::from(value("ssh_private_key_file"));
    assert!(private.is_file() && private.ends_with("ssh/id_ed25519"), "{values:?}");
    assert_eq!(run.key_values().await.unwrap(), values, "the same key each time");
    let _ = std::fs::remove_dir_all(&home);
}
