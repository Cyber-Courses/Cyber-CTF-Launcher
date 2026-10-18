//! Cloud-services labs (runtime `CLOUD`): no machines, but cloud services (IAM users, functions,
//! buckets, instances...) from a Terraform module of the lab's own (`cloud:` in its
//! `isoloom.yml`, Isoloom's `cloud-services` target), applied into the player's cloud account.
//!
//! They reuse what the other cloud labs have: the player's cloud account of the lab's cloud
//! (its credentials, region and auto-stop), the host Terraform, the AWS budget guard, and the
//! state kept under `<app data>/deployments/<lab>/<cloud>/`, outside the lab folder, with a run
//! record whose `expires_at` the reaper (`reap_expired_labs`) destroys the lab at. What differs:
//!
//! - The module is the lab's, used as is (see `terraform::services`), with Isoloom's
//!   generated `terraform.tfvars.json` (the spec's fixed `cloud.vars`).
//! - Variables holding `{{ inputs.NAME }}` (a whole value, or inside a string at any depth:
//!   `["{{ inputs.PLAYER_IP }}/32"]`) take launch-time inputs, written to
//!   `inputs.tfvars.json` (mode 0600) beside the state, so the destroy reuses them, as Isoloom's
//!   CLI does. The launcher fills `PLAYER_CIDR` (this machine's public IP as a /32, for an
//!   allow-list) and `PLAYER_IP`, and the launch's own values (`CTF_API_URL`,
//!   `CTF_LAUNCH_TOKEN`); a lab asking for anything else can't start here.
//! - Every module may take `expires_at` (Unix seconds, for its tags) and `region`: passed as
//!   `TF_VAR_*`, which Terraform ignores for a module that doesn't declare them.
//! - On AWS, Terraform also gets shared credentials and config files of its own (see
//!   `AwsFiles`): labs that name a profile (CloudGoat's `profile = var.profile`, and
//!   `--profile` in its local-exec commands) find the picked account's keys under it, since an
//!   explicitly set profile beats the environment's keys in the AWS provider.
//! - `cloud.env` goes into Terraform's environment, over the account's own (a lab's
//!   `AWS_REGION`), and `cloud.apply_retries` applies again after a failed apply. What
//!   `cloud.requires` names must be on this machine. `cloud.before` (set-up steps run on this
//!   machine before Terraform) isn't supported yet: such a lab is refused before anything is
//!   created.
//! - What the lab exposes is its outputs (`cloud.outputs`), read from the state; its message
//!   names them (`{{ cloud.outputs.NAME }}`). Stop destroys everything: nothing is parked.

use std::path::Path;

use crate::AppHandle;

use super::lab::{CloudProvider, CloudServices, Spec, Target};
use super::launch::{CLOUD_NEEDS_ACCOUNT, check_aws_budget, public_ip};
use super::paths::lab_dir;
use super::providers::Provider;
use super::{Runtime, isothrum, lab, server};
use crate::error::{Error, Result};

/// The launch-time input the launcher fills with this machine's public IP as a /32.
pub const PLAYER_CIDR: &str = "PLAYER_CIDR";
/// The launch-time input the launcher fills with this machine's public IP.
pub const PLAYER_IP: &str = "PLAYER_IP";

/// The lab's cloud services, or why it has none.
pub fn of(spec: &Spec) -> Result<&CloudServices> {
    spec.cloud.as_ref().ok_or_else(|| Error::Invalid("this lab's isoloom.yml declares no cloud services (`cloud:`)".into()))
}

/// The launcher's provider for a cloud.
pub fn provider(cloud: CloudProvider) -> Provider {
    match cloud {
        CloudProvider::Aws => Provider::Aws,
        CloudProvider::Azure => Provider::Azure,
        CloudProvider::Gcp => Provider::Gcp,
    }
}

/// The launch-time inputs the lab's variables take (each `{{ inputs.NAME }}`, at any depth), as
/// (variable, input) pairs.
fn wanted(cloud: &CloudServices) -> Vec<(&str, String)> {
    cloud.vars.iter().flat_map(|(var, v)| CloudServices::inputs_in(v).into_iter().map(move |i| (var.as_str(), i))).collect()
}

/// Whether a launch needs this machine's public IP (an allow-list input).
pub fn needs_player_ip(cloud: &CloudServices) -> bool {
    wanted(cloud).iter().any(|(_, i)| i == PLAYER_CIDR || i == PLAYER_IP)
}

/// "About $0.02 an hour, $0.08 until it auto-stops in 4 h", for the log and the launch dialog.
pub fn cost_line(hourly_usd: Option<f64>, auto_stop_hours: u32) -> String {
    match hourly_usd {
        Some(h) if auto_stop_hours > 0 => format!(
            "Estimated cost: about ${h:.2} an hour, so about ${:.2} if it runs until it auto-stops in {auto_stop_hours} h (the lab's own estimate; your cloud bills the real amount).",
            h * f64::from(auto_stop_hours)
        ),
        Some(h) => format!("Estimated cost: about ${h:.2} an hour until you stop it (the lab's own estimate; your cloud bills the real amount)."),
        None => "This lab gives no cost estimate; your cloud bills what it creates until you stop it.".into(),
    }
}

/// Starts a cloud-services lab on the player's cloud account `host`, through the Isothrum runner
/// (the run manifest Isoloom writes beside the lab's own Terraform module). The launcher keeps the
/// policy — the account is of the lab's cloud, the GCP labs project, the AWS budget, this machine's
/// public IP where the lab asks for it — and the runner runs the module, keeps the state and the
/// expiry, and ends what it costs on a stop or the auto-stop.
pub(super) async fn start(app: &AppHandle, dir: &Path, id: &str, host: Option<&str>, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    let host = host.ok_or_else(|| Error::Invalid(CLOUD_NEEDS_ACCOUNT.into()))?;
    let spec = lab::prepare(dir, Target::CloudServices)?.spec;
    let cloud = of(&spec)?;
    check_supported(cloud)?;
    let conn = server::connection(app, host)?;
    check_account(cloud.provider, &conn)?;
    let conn = ready_account(app, host, conn, log).await?;
    // Mark first, so a half-created lab can still be destroyed with the same account.
    server::mark_lab(dir, Some(host))?;
    log(format!("Running in your {} account {} ({})", conn.provider.id().to_uppercase(), conn.name, region_used(cloud, &conn).unwrap_or("default region")));
    if conn.provider == Provider::Aws {
        check_aws_budget(app, host, log).await?;
    }
    let hours = auto_stop_hours(&conn);
    log(cost_line(cloud.hourly_usd, hours));
    // The lab's launch inputs, and this machine's public IP where the lab asks for an allow-list.
    let mut inputs = env.to_vec();
    if let Some(ip) = player_ip(cloud, log).await? {
        inputs.push(("PLAYER_IP".to_string(), ip));
    }
    log(format!("Creating the lab's cloud services ({})…", cloud.terraform));
    let place = isothrum::CloudPlace {
        label: host.to_string(),
        region: conn.region.clone(),
        tf_env: conn.tf_env.clone(),
        lifetime: (hours > 0).then(|| std::time::Duration::from_secs(u64::from(hours) * 3600)),
        ..isothrum::CloudPlace::default()
    };
    let cloud_tf = server::terraform_target(conn.provider).unwrap_or_default();
    isothrum::CloudRun::new(&super::paths::runner_home(app)?, dir, id, Runtime::Cloud, cloud_tf)?.up(&place, &inputs, log).await
}

/// Refuses a lab using what this launcher doesn't run yet: set-up steps on this machine before
/// Terraform (`cloud.before`).
fn check_supported(cloud: &CloudServices) -> Result<()> {
    if !cloud.before.is_empty() {
        return Err(Error::Invalid(format!(
            "This lab runs set-up steps on this machine before creating its cloud services ({}), which Cyber CTF can't run yet. Nothing was created.",
            cloud.before.join(", ")
        )));
    }
    let reserved: Vec<&str> = cloud.env.keys().map(String::as_str).filter(|k| reserved_env(k)).collect();
    if !reserved.is_empty() {
        return Err(Error::Invalid(format!(
            "This lab sets {} in its environment (`cloud.env`), which Cyber CTF keeps to itself: the cloud account's credentials and Terraform's own settings come from the launcher. Nothing was created.",
            reserved.join(", ")
        )));
    }
    Ok(())
}

/// The environment variables a lab's `cloud.env` may not set (Isoloom refuses only its scripts'
/// and the shell's own names): the cloud accounts' credentials and sign-in (every `AWS_*` but
/// the region, `ARM_*`, `AZURE_*`, `GOOGLE_*`, `CLOUDSDK_*`, `OCI_*`, the DigitalOcean and Linode
/// tokens) and Terraform's own (`TF_*`: `TF_VAR_*` would beat the launcher's variables,
/// `TF_CLI_ARGS*` change what an apply or destroy does, `TF_LOG*` write files). The launcher
/// sets those from the picked account; a lab may still pick its region (`AWS_REGION`).
fn reserved_env(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    if n == "AWS_REGION" || n == "AWS_DEFAULT_REGION" {
        return false;
    }
    ["AWS_", "ARM_", "AZURE_", "GOOGLE_", "CLOUDSDK_", "OCI_", "TF_"].iter().any(|p| n.starts_with(p)) || n == "DIGITALOCEAN_TOKEN" || n == "LINODE_TOKEN"
}

/// The region a run uses: the lab's own (`AWS_REGION` in `cloud.env`), else the account's.
fn region_used<'a>(cloud: &'a CloudServices, conn: &'a server::Connection) -> Option<&'a str> {
    ["AWS_REGION", "AWS_DEFAULT_REGION"].iter().find_map(|k| cloud.env.get(*k)).map(String::as_str).or(conn.region.as_deref())
}

/// Refuses an account of another cloud than the lab's.
fn check_account(cloud: CloudProvider, conn: &server::Connection) -> Result<()> {
    if conn.provider == provider(cloud) {
        return Ok(());
    }
    let wanted = cloud.id().to_uppercase();
    Err(Error::Invalid(format!("This lab runs on {wanted}, and {} is a {} account. Pick a {wanted} account.", conn.name, conn.provider.id().to_uppercase())))
}

/// The account ready to run labs in: on GCP, its labs project made sure of (one free billing
/// slot), and the connection read again with it.
async fn ready_account(app: &AppHandle, host: &str, conn: server::Connection, log: &mut impl FnMut(String)) -> Result<server::Connection> {
    if conn.provider != Provider::Gcp {
        return Ok(conn);
    }
    log("Checking the Cyber CTF labs project on Google Cloud…".into());
    server::gcp::labs_project(app, host).await?;
    server::connection(app, host)
}

/// The account's auto-stop, in hours (0: none).
fn auto_stop_hours(conn: &server::Connection) -> u32 {
    conn.auto_stop_hours.unwrap_or(server::DEFAULT_AUTO_STOP_HOURS)
}

/// This machine's public IP when the lab lets only it in somewhere (an allow-list input).
async fn player_ip(cloud: &CloudServices, log: &mut impl FnMut(String)) -> Result<Option<String>> {
    if !needs_player_ip(cloud) {
        return Ok(None);
    }
    let ip = public_ip().await?;
    log(format!("Only your public IP ({ip}) is let in where the lab asks for an allow-list."));
    Ok(Some(ip))
}

/// What the launch dialog shows before a cloud-services lab starts: its cloud, what it costs
/// and what it asks for. Read from the lab's spec at its pinned commit.
#[derive(serde::Serialize, Debug, PartialEq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "CloudPreview")]
pub struct Preview {
    pub provider: String,
    pub hourly_usd: Option<f64>,
    /// The launch-time inputs it takes.
    pub inputs: Vec<String>,
    /// Whether it opens something to this machine's public IP only.
    pub allow_list: bool,
    /// The names of what it exposes once up.
    pub outputs: Vec<String>,
}

pub fn preview(spec: &Spec) -> Option<Preview> {
    let cloud = spec.cloud.as_ref()?;
    Some(Preview {
        provider: cloud.provider.id().to_string(),
        hourly_usd: cloud.hourly_usd,
        inputs: wanted(cloud).into_iter().map(|(_, i)| i).fold(Vec::new(), |mut all, i| {
            if !all.contains(&i) {
                all.push(i);
            }
            all
        }),
        allow_list: needs_player_ip(cloud),
        outputs: cloud.outputs.keys().cloned().collect(),
    })
}

/// [`Preview`] of a lab at a commit: the installed copy when it is that commit, else its
/// `isoloom.yml` from GitHub (CyberCTF repositories at a full SHA only).
pub async fn preview_at(app: &AppHandle, lab_id: &str, repository: &str, commit: &str) -> Result<Option<Preview>> {
    super::validate_id(lab_id)?;
    crate::labs::validate_source(repository, commit)?;
    if let Ok(dir) = lab_dir(app, lab_id)
        && std::fs::read_to_string(dir.join(".cyberctf-commit")).is_ok_and(|c| c.trim() == commit)
        && let Ok(spec) = lab::load(&dir)
    {
        return Ok(preview(&spec));
    }
    let url = format!("https://raw.githubusercontent.com/{repository}/{commit}/isoloom.yml");
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().map_err(|e| Error::Invalid(e.to_string()))?;
    let res = client.get(&url).send().await.map_err(|e| Error::Invalid(format!("couldn't read the lab's spec: {e}")))?;
    if !res.status().is_success() {
        return Err(Error::Invalid(format!("couldn't read the lab's spec: HTTP {}", res.status())));
    }
    let text = res.text().await.map_err(|e| Error::Invalid(format!("couldn't read the lab's spec: {e}")))?;
    let spec = lab::parse(&text)?;
    Ok(preview(&spec))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "version: 1\nname: goat\ninputs: [PLAYER_CIDR]\ncloud:\n  provider: aws\n  terraform: terraform\n  vars: { region: us-east-1, whitelist: \"{{ inputs.PLAYER_CIDR }}\", size: 2 }\n  outputs: { key: access_key_id, secret: secret_key, app: app_url }\n  hourly_usd: 0.02\nmessage: |\n  Sign in with {{ cloud.outputs.key }} at {{ cloud.outputs.app }}.\n";

    fn spec(yaml: &str) -> Spec {
        lab::parse(yaml).expect("parses")
    }

    #[test]
    fn the_catalogues_runtimes_read_as_launcher_runtimes() {
        for (json, rt) in [("\"DOCKER\"", Runtime::Docker), ("\"VM\"", Runtime::Vm), ("\"CLOUD\"", Runtime::Cloud)] {
            assert_eq!(serde_json::from_str::<Runtime>(json).unwrap(), rt);
            assert_eq!(serde_json::to_string(&rt).unwrap(), json);
        }
        assert!(serde_json::from_str::<Runtime>("\"HOSTED\"").is_err());
    }

    /// isoloom #127: an input inside a string at any depth (CloudGoat's allow-list), the lab's own
    /// environment, more applies, and what a lab needs here.
    const GOAT_TEMPLATE: &str = "version: 1\nname: goat\ninputs: [PLAYER_IP]\ncloud:\n  provider: aws\n  terraform: terraform\n  vars: { cg_whitelist: [\"{{ inputs.PLAYER_IP }}/32\"], tag: \"lab-{{ inputs.PLAYER_IP }}-{{ inputs.PLAYER_IP }}\", region: us-east-1 }\n  env: { AWS_REGION: us-east-1 }\n  apply_retries: 1\n  requires: [{TOOL}]\n";

    /// A command every machine the tests run on has.
    const TOOL: &str = if cfg!(windows) { "cmd" } else { "sh" };

    /// [`GOAT_TEMPLATE`] needing [`TOOL`].
    fn goat() -> String {
        GOAT_TEMPLATE.replace("{TOOL}", TOOL)
    }

    #[test]
    fn set_up_steps_on_this_machine_are_refused_before_anything_runs() {
        assert!(check_supported(of(&spec(&goat())).unwrap()).is_ok());
        let s = spec(&format!("{}  before: [keys.sh]\n", goat()));
        let err = check_supported(of(&s).unwrap()).unwrap_err().to_string();
        assert!(err.contains("(keys.sh)") && err.contains("Nothing was created"), "{err}");
    }

    #[tokio::test]
    async fn a_lab_with_set_up_steps_is_refused_before_the_account_is_read() {
        use crate::platform::exec::fake::Tools;
        use crate::testing::test_app::TestApp;
        let t = TestApp::new();
        let dir = t.lab(
            "goat",
            &[("isoloom.yml", format!("{}  before: [keys.sh]\n", goat()).as_str()), ("keys.sh", "true\n"), ("terraform/main.tf", "variable \"region\" {}\n")],
        );
        let tools = Tools::rules(&[]);
        // No such account: had it been read first, that would be the error.
        let err = super::super::start(t.handle(), &dir, "goat", Runtime::Cloud, None, Some("no-such-account"), &[], |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("set-up steps on this machine"), "{err}");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(!t.data().join("deployments/goat").exists());
    }

    #[test]
    fn cloud_accounts_match_by_provider() {
        assert_eq!(provider(CloudProvider::Aws), Provider::Aws);
        assert_eq!(provider(CloudProvider::Azure), Provider::Azure);
        assert_eq!(provider(CloudProvider::Gcp), Provider::Gcp);
    }

    #[test]
    fn cost_lines() {
        assert!(cost_line(Some(0.02), 4).contains("$0.02 an hour, so about $0.08"));
        assert!(cost_line(Some(0.5), 0).contains("until you stop it"));
        assert!(cost_line(None, 4).contains("no cost estimate"));
    }

    #[tokio::test]
    async fn the_preview_command_reads_the_installed_copy_at_that_commit() {
        let t = crate::testing::test_app::TestApp::new();
        let commit = "0123456789abcdef0123456789abcdef01234567";
        t.lab("goat", &[("isoloom.yml", SPEC), ("terraform/main.tf", "variable \"region\" {}\n"), (".cyberctf-commit", commit)]);
        let app = || t.handle().clone();
        let got = super::super::lab_cloud_preview(app(), "goat".into(), "CyberCTF/goat".into(), commit.into()).await.unwrap();
        assert_eq!(got, preview(&spec(SPEC)));
        assert_eq!(got.map(|p| p.provider).as_deref(), Some("aws"));
        // A lab that isn't a cloud-services one has no preview.
        let docker =
            "version: 1\nname: web\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n";
        t.lab("web", &[("isoloom.yml", docker), (".cyberctf-commit", commit)]);
        assert_eq!(super::super::lab_cloud_preview(app(), "web".into(), "CyberCTF/web".into(), commit.into()).await.unwrap(), None);
        // Refused before anything is downloaded: a bad lab id, another owner, a short commit.
        let refuse = |id: &str, repo: &str, commit: &str| super::super::lab_cloud_preview(app(), id.into(), repo.into(), commit.into());
        assert_eq!(refuse("../x", "CyberCTF/goat", commit).await.unwrap_err().to_string(), "invalid lab id `../x`");
        assert_eq!(refuse("goat", "evil/goat", commit).await.unwrap_err().to_string(), format!("refusing to download evil/goat@{commit}"));
        assert_eq!(refuse("goat", "CyberCTF/goat", "main").await.unwrap_err().to_string(), "refusing to download CyberCTF/goat@main");
    }
}
