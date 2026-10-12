//! Cloud account connection via each provider's own CLI auth (browser flow), so we don't
//! store long-lived cloud secrets. Terraform then uses the CLI's credential chain.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;

use crate::error::Result;
use crate::exec::{run, stream};

/// Civil (year, month, day) for a Unix timestamp, UTC (Howard Hinnant's algorithm). Used to
/// build Cost Explorer date ranges without a date-crate dependency.
fn ymd_from_secs(secs: i64) -> (i64, u32, u32) {
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cloud {
    Aws,
    Azure,
    Gcp,
}

/// Signs in to a cloud provider using its CLI. AWS uses SSO (needs a configured SSO
/// profile); Azure and GCP open the browser. Streams the CLI output to the UI.
#[tauri::command]
pub async fn cloud_login(provider: Cloud, logs: Channel<String>) -> Result<()> {
    let on_line = move |line: String| {
        let _ = logs.send(line);
    };
    let (program, args) = login_command(&provider);
    on_line(format!("$ {program} {}", args.join(" ")));
    stream(program, args, None, &[], on_line).await
}

/// The CLI sign-in for a cloud.
fn login_command(provider: &Cloud) -> (&'static str, &'static [&'static str]) {
    match provider {
        Cloud::Aws => ("aws", &["sso", "login"]),
        Cloud::Azure => ("az", &["login"]),
        // One browser flow that authenticates the gcloud CLI (so `gcloud projects list` works)
        // AND writes Application Default Credentials (what Terraform's google provider reads).
        // Plain `auth application-default login` only does the latter, leaving the CLI unauthed.
        Cloud::Gcp => ("gcloud", &["auth", "login", "--update-adc"]),
    }
}

/// `args`, then `--profile <p>` when a profile is given.
fn with_profile<'a>(mut args: Vec<&'a str>, profile: Option<&'a str>) -> Vec<&'a str> {
    if let Some(p) = profile {
        args.push("--profile");
        args.push(p);
    }
    args
}

/// A CLI's one-line answer, trimmed; None when blank.
fn non_empty(out: String) -> Option<String> {
    Some(out.trim().to_string()).filter(|s| !s.is_empty())
}

/// The identity the host AWS CLI resolves (for the given profile, or the default chain),
/// if any, so the cloud setup can show "signed in as …" and offer using the CLI instead of
/// pasting keys. None when the CLI is missing or that profile has no usable credentials.
#[tauri::command]
pub async fn aws_cli_identity(profile: Option<String>) -> Option<String> {
    let args = with_profile(vec!["sts", "get-caller-identity", "--query", "Arn", "--output", "text"], profile.as_deref());
    run("aws", &args, None).await.ok().and_then(non_empty)
}

/// The Cost Explorer time period for this month so far: Start = the 1st, End = tomorrow
/// (End is exclusive, so tomorrow includes today's spend).
pub(crate) fn month_period() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, m, _) = ymd_from_secs(now);
    let (ey, em, ed) = ymd_from_secs(now + 86400);
    format!("Start={y:04}-{m:02}-01,End={ey:04}-{em:02}-{ed:02}")
}

/// This month's AWS spend so far in USD, from Cost Explorer, for the budget check. None
/// when the CLI or Cost Explorer isn't available (CE must be enabled on the account).
#[tauri::command]
pub async fn aws_month_to_date_cost(profile: Option<String>) -> Option<f64> {
    let period = month_period();
    let args = with_profile(cost_args(&period), profile.as_deref());
    run("aws", &args, None).await.ok().and_then(|s| s.trim().parse::<f64>().ok())
}

/// `aws ce get-cost-and-usage` for `period`, printing just the amount.
fn cost_args(period: &str) -> Vec<&str> {
    vec![
        "ce",
        "get-cost-and-usage",
        "--time-period",
        period,
        "--granularity",
        "MONTHLY",
        "--metrics",
        "UnblendedCost",
        "--query",
        "ResultsByTime[0].Total.UnblendedCost.Amount",
        "--output",
        "text",
    ]
}

/// The AWS CLI profiles configured on this machine (`aws configure list-profiles`), so the
/// user can pick one when they have several. Empty when the CLI is missing or has none.
#[tauri::command]
pub async fn aws_profiles() -> Vec<String> {
    run("aws", &["configure", "list-profiles"], None).await.ok().map(|s| lines(&s)).unwrap_or_default()
}

/// The non-blank lines of a CLI's output, trimmed.
fn lines(out: &str) -> Vec<String> {
    out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// An Azure subscription the signed-in account can use.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AzureSubscription {
    pub name: String,
    pub id: String,
    #[serde(default)]
    pub is_default: bool,
}

/// The Azure subscriptions the signed-in account can see (`az account list`), so the user can
/// pick one instead of pasting a GUID. Empty when the CLI is missing or not signed in.
#[tauri::command]
pub async fn azure_subscriptions() -> Vec<AzureSubscription> {
    run("az", &["account", "list", "--query", "[].{name:name,id:id,isDefault:isDefault}", "--output", "json"], None)
        .await
        .ok()
        .map(|out| subscriptions(&out))
        .unwrap_or_default()
}

/// `az account list` output (the query above) as subscriptions; none when it doesn't parse.
fn subscriptions(out: &str) -> Vec<AzureSubscription> {
    serde_json::from_str::<Vec<AzureSubscription>>(out).unwrap_or_default()
}

/// A GCP billing account the signed-in user can see.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GcpBillingAccount {
    pub id: String,
    pub name: String,
    pub open: bool,
}

/// A GCP organization the signed-in user belongs to.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GcpOrganization {
    pub id: String,
    pub name: String,
}

/// What the launcher found in ~/.oci/config (DEFAULT profile), to prefill the OCI setup.
#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OciConfig {
    pub configured: bool,
    pub tenancy: String,
    pub region: String,
}

/// Reads the OCI config (OCI_CLI_CONFIG_FILE, else ~/.oci/config) DEFAULT profile, so the setup
/// can show whether it's configured and prefill the tenancy (as the compartment) and region.
#[tauri::command]
pub async fn oci_config() -> OciConfig {
    read_oci_config(oci_config_path().as_deref())
}

/// OCI_CLI_CONFIG_FILE, else ~/.oci/config.
fn oci_config_path() -> Option<std::path::PathBuf> {
    std::env::var_os("OCI_CLI_CONFIG_FILE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| std::path::PathBuf::from(h).join(".oci").join("config")))
}

/// The config file at `path`; not configured when there is none or it can't be read.
fn read_oci_config(path: Option<&std::path::Path>) -> OciConfig {
    let Some(path) = path else { return OciConfig::default() };
    let Ok(text) = std::fs::read_to_string(path) else { return OciConfig::default() };
    parse_oci_config(&text)
}

/// The DEFAULT profile's tenancy and region in an OCI config file's text.
fn parse_oci_config(text: &str) -> OciConfig {
    let (mut tenancy, mut region) = (String::new(), String::new());
    let mut in_default = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_default = l.eq_ignore_ascii_case("[DEFAULT]");
            continue;
        }
        if !in_default {
            continue;
        }
        if let Some((k, val)) = l.split_once('=') {
            match k.trim() {
                "tenancy" => tenancy = val.trim().to_string(),
                "region" => region = val.trim().to_string(),
                _ => {}
            }
        }
    }
    OciConfig { configured: true, tenancy, region }
}

/// The active gcloud account email, or None if the CLI isn't signed in. Reliable even when
/// `projects list` is empty or the Resource Manager API is off, so the UI can show "signed in".
/// An account whose sign-in expired (Workspace reauthentication) still shows as active in
/// `auth list` while every call fails: it counts as signed out, so the UI offers to sign in
/// again instead of showing an empty billing list.
#[tauri::command]
pub async fn gcp_account() -> Option<String> {
    let email = run("gcloud", &["auth", "list", "--filter=status:ACTIVE", "--format=value(account)"], None).await.ok().and_then(non_empty)?;
    run("gcloud", &["auth", "print-access-token", "--quiet"], None).await.ok()?;
    Some(email)
}

/// The GCP billing accounts the signed-in user can see (`gcloud billing accounts list`), so they
/// pick one for the per-lab projects. Empty when the CLI is missing or not signed in.
#[tauri::command]
pub async fn gcp_billing_accounts() -> Vec<GcpBillingAccount> {
    run("gcloud", &["billing", "accounts", "list", "--format", "json"], None).await.ok().map(|out| billing_accounts(&out)).unwrap_or_default()
}

/// The entries of a gcloud JSON list; none when it doesn't parse.
fn json_list(out: &str) -> Vec<serde_json::Value> {
    serde_json::from_str::<Vec<serde_json::Value>>(out).unwrap_or_default()
}

/// `gcloud billing accounts list --format json` as billing accounts (ids without their prefix).
fn billing_accounts(out: &str) -> Vec<GcpBillingAccount> {
    json_list(out)
        .into_iter()
        .map(|v| GcpBillingAccount {
            id: v["name"].as_str().unwrap_or_default().trim_start_matches("billingAccounts/").to_string(),
            name: v["displayName"].as_str().unwrap_or_default().to_string(),
            open: v["open"].as_bool().unwrap_or(false),
        })
        .filter(|b| !b.id.is_empty())
        .collect()
}

/// The GCP organizations the signed-in user belongs to (`gcloud organizations list`). Empty for
/// a personal / no-org account, so the user can create projects without a parent.
#[tauri::command]
pub async fn gcp_organizations() -> Vec<GcpOrganization> {
    run("gcloud", &["organizations", "list", "--format", "json"], None).await.ok().map(|out| organizations(&out)).unwrap_or_default()
}

/// `gcloud organizations list --format json` as organizations (ids without their prefix).
fn organizations(out: &str) -> Vec<GcpOrganization> {
    json_list(out)
        .into_iter()
        .map(|v| GcpOrganization {
            id: v["name"].as_str().unwrap_or_default().trim_start_matches("organizations/").to_string(),
            name: v["displayName"].as_str().unwrap_or_default().to_string(),
        })
        .filter(|o| !o.id.is_empty())
        .collect()
}

/// Signs in to AWS in the browser (`aws login`, AWS CLI >= 2.32.0): console credentials for
/// root / IAM / federation, temporary credentials for up to 12 hours, no SSO setup needed.
/// Streams the CLI output.
#[tauri::command]
pub async fn aws_login(profile: Option<String>, logs: Channel<String>) -> Result<()> {
    let on_line = move |line: String| {
        let _ = logs.send(line);
    };
    let args = with_profile(vec!["login"], profile.as_deref());
    on_line(format!("$ aws {}", args.join(" ")));
    stream("aws", &args, None, &[], on_line).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::fake::{Tools, fail, ok};

    fn sink() -> (Channel<String>, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let s = seen.clone();
        let channel = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(j) = body {
                s.lock().unwrap().push(serde_json::from_str(&j).unwrap());
            }
            Ok(())
        });
        (channel, seen)
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
        let _ = aws_month_to_date_cost(None).await;
        assert_eq!(azure_subscriptions().await.len(), 1);
        assert_eq!(gcp_account().await.as_deref(), Some("ada@example.com"));
        let _ = gcp_billing_accounts().await;
        let _ = gcp_organizations().await;
        let (channel, seen) = sink();
        cloud_login(Cloud::Azure, channel).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), ["$ az login", "signed in"]);
        let (channel, seen) = sink();
        aws_login(Some("labs".into()), channel).await.unwrap();
        assert_eq!(seen.lock().unwrap()[0], "$ aws login --profile labs");
    }

    #[tokio::test]
    async fn signed_out_clis_read_as_nothing() {
        let _tools = Tools::new(|_, args| match args {
            ["auth", "list", ..] => ok("ada@example.com\n"),
            _ => fail("expired"),
        });
        assert_eq!(aws_cli_identity(None).await, None);
        assert!(aws_profiles().await.is_empty());
        assert_eq!(aws_month_to_date_cost(None).await, None);
        assert!(azure_subscriptions().await.is_empty());
        assert_eq!(gcp_account().await, None, "an expired sign-in counts as signed out");
        assert!(gcp_billing_accounts().await.is_empty() && gcp_organizations().await.is_empty());
        assert!(cloud_login(Cloud::Gcp, sink().0).await.is_err());
    }

    #[test]
    fn each_cloud_signs_in_with_its_own_cli() {
        assert_eq!(login_command(&Cloud::Aws), ("aws", &["sso", "login"][..]));
        assert_eq!(login_command(&Cloud::Azure), ("az", &["login"][..]));
        // gcloud's login also writes the credentials Terraform reads.
        assert_eq!(login_command(&Cloud::Gcp), ("gcloud", &["auth", "login", "--update-adc"][..]));
    }

    #[test]
    fn a_profile_is_passed_only_when_chosen() {
        assert_eq!(with_profile(vec!["login"], None), ["login"]);
        assert_eq!(with_profile(vec!["login"], Some("work")), ["login", "--profile", "work"]);
        let cost = with_profile(cost_args("Start=2026-10-01,End=2026-10-11"), Some("p"));
        assert_eq!(cost[..4], ["ce", "get-cost-and-usage", "--time-period", "Start=2026-10-01,End=2026-10-11"]);
        assert_eq!(cost[cost.len() - 2..], ["--profile", "p"]);
    }

    #[test]
    fn cli_answers_are_trimmed_lines() {
        assert_eq!(non_empty(" arn:aws:iam::1:user/a \n".into()).as_deref(), Some("arn:aws:iam::1:user/a"));
        assert_eq!(non_empty("  \n".into()), None);
        assert_eq!(lines("default\n  work \n\n"), ["default", "work"]);
        assert!(lines("").is_empty());
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
        assert_eq!(oci_config().await, read_oci_config(oci_config_path().as_deref()));
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
    fn civil_date_from_unix_timestamp() {
        // Epoch.
        assert_eq!(ymd_from_secs(0), (1970, 1, 1));
        // A known instant: 2021-01-01 00:00:00 UTC = 1609459200.
        assert_eq!(ymd_from_secs(1_609_459_200), (2021, 1, 1));
        // Leap day: 2020-02-29 12:00:00 UTC = 1582977600.
        assert_eq!(ymd_from_secs(1_582_977_600), (2020, 2, 29));
        // End of a year: 2023-12-31 23:59:59 UTC = 1704067199.
        assert_eq!(ymd_from_secs(1_704_067_199), (2023, 12, 31));
        // The day before epoch (negative seconds).
        assert_eq!(ymd_from_secs(-1), (1969, 12, 31));
    }

    #[test]
    fn month_period_spans_the_first_to_an_exclusive_end() {
        // Cost Explorer wants Start=<1st of this month>,End=<tomorrow> (End exclusive).
        let p = month_period();
        assert!(p.starts_with("Start="), "{p}");
        let (start, end) = p.split_once(",End=").unwrap();
        let start = start.strip_prefix("Start=").unwrap();
        // Start is always day 01 of a month; both are YYYY-MM-DD.
        assert!(start.ends_with("-01"), "{start}");
        assert_eq!(start.len(), 10);
        assert_eq!(end.len(), 10);
        // End is strictly after Start (this month has at least one day so far).
        assert!(end > start, "end {end} should be after start {start}");
    }
}
