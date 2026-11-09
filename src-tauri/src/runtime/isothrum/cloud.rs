//! A lab in the player's cloud account through the Isothrum runner, as VMs (`cloud-vm`) or as
//! its containers in a VM (`cloud-docker`): the run manifest Isoloom writes beside its Terraform
//! module (`.isoloom[-<n>]/cloud-vm/<cloud>/run.json`,
//! `.isoloom[-<n>]/cloud-docker/<cloud>/run.json`), run through the runner's operations instead
//! of the launcher's own Terraform path.
//!
//! What stays the launcher's (policy, never execution): the run's name (`cyberctf-<lab id>`), the
//! AWS budget guard, this machine's public IP for the firewall, the GCP labs project, and turning
//! the player's cloud account into the module's inputs. The runner keeps the Terraform state, the
//! run's record and the expiry a destroy replays; it reads the module's `ssh_user` output and
//! waits on the project delivery, so a cloud lab now actually comes up, is sealed and checked.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use isothrum_runner::{RunName, RunOptions, Runner, RunnerConfig, SecretValue, Secrets, SystemProcesses};

use super::opened::OpenedRun;
use super::{errors, progress, view};
use crate::error::{Error, Result};
use crate::runtime::docker::Check;
use crate::runtime::{LabStatus, Place, Runtime, lab};

/// The account environment names the launcher passes to the module as **parameters** (not
/// secrets): a chosen AWS CLI profile and the region. Everything else in the account's Terraform
/// environment is a credential, delivered as a connection secret.
const PARAM_ENV: [&str; 3] = ["AWS_PROFILE", "AWS_REGION", "AWS_DEFAULT_REGION"];

/// Where a cloud lab runs, as the launcher resolved it from the player's account.
#[derive(Debug, Clone, Default)]
pub(crate) struct CloudPlace {
    /// The account's id among the launcher's hosts (recorded: a later operation elsewhere is
    /// refused).
    pub(crate) label: String,
    /// The account's region, as the module's `region` parameter.
    pub(crate) region: Option<String>,
    /// This machine's public IP as a CIDR (`<ip>/32`), the module's `allowed_cidr`: only this
    /// machine reaches the lab's SSH and published ports.
    pub(crate) allowed_cidr: Option<String>,
    /// The account's Terraform environment (`conn.tf_env`): its credentials (delivered as the
    /// manifest's connection secrets) and any profile/region (delivered as parameters).
    pub(crate) tf_env: Vec<(String, String)>,
    /// When the run stops itself (the account's auto-stop): the manifest's `expiry`.
    pub(crate) lifetime: Option<Duration>,
}

impl CloudPlace {
    /// The run's connection secrets: every credential in the account's environment (a value no
    /// manifest slot takes is never delivered), by its environment-variable name.
    fn secrets(&self, env: &[(String, String)]) -> Secrets {
        let mut secrets = Secrets::new();
        for (k, v) in self.tf_env.iter().filter(|(k, _)| !PARAM_ENV.contains(&k.as_str())) {
            secrets.insert(k.clone(), SecretValue::new(v.clone()));
        }
        // The launch's values for the lab's input slots (a flag, a launch token).
        for (k, v) in env {
            secrets.insert(k.clone(), SecretValue::new(v.clone()));
        }
        secrets
    }

    /// The module's parameters: the region, the firewall CIDR, and a chosen AWS CLI profile.
    fn params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();
        if let Some(region) = &self.region {
            params.push(("region".to_owned(), region.clone()));
        }
        if let Some(cidr) = &self.allowed_cidr {
            params.push(("allowed_cidr".to_owned(), cidr.clone()));
        }
        if let Some((_, profile)) = self.tf_env.iter().find(|(k, _)| k == "AWS_PROFILE") {
            params.push(("AWS_PROFILE".to_owned(), profile.clone()));
        }
        params
    }

    /// The options of an operation on this account, with the launch's values `env`.
    pub(crate) fn options(&self, env: &[(String, String)]) -> RunOptions {
        let opts = RunOptions::new().with_secrets(self.secrets(env)).with_params(self.params()).with_host_label(&self.label);
        match self.lifetime {
            Some(lifetime) => opts.with_lifetime(lifetime),
            None => opts,
        }
    }
}

/// The question that tells whether `cloud`'s sign-in still holds, asked of its CLI: who AWS's
/// credentials are, an Azure CLI token, Google's application-default token (what its Terraform
/// provider uses), Oracle's regions (through `~/.oci/config`). `DigitalOcean` and Linode sign in
/// with a stored API token, which doesn't run out: none.
fn sign_in_question(cloud: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match cloud {
        "aws" => ("aws", &["sts", "get-caller-identity", "--output", "text"]),
        "azure" => ("az", &["account", "get-access-token", "--output", "none"]),
        "gcp" => ("gcloud", &["auth", "application-default", "print-access-token"]),
        "oci" => ("oci", &["iam", "region", "list", "--output", "json"]),
        _ => return None,
    })
}

/// How long the sign-in question may take.
const SIGN_IN_WAIT: Duration = Duration::from_secs(30);

/// One cloud lab (`cloud-vm` or `cloud-docker`), run by the runner.
pub(crate) struct CloudRun {
    run: OpenedRun,
    /// The launcher's Terraform target for the cloud (`aws`, `azure`, …).
    cloud: String,
}

/// The run's opened manifest and what reads it (its status, console, update and retire) are
/// [`OpenedRun`]'s, shared with the other remote place (`ServerRun`).
impl std::ops::Deref for CloudRun {
    type Target = OpenedRun;
    fn deref(&self) -> &OpenedRun {
        &self.run
    }
}

impl CloudRun {
    /// Lab `id` of `runtime` (a container lab is `cloud-docker`, a VM lab `cloud-vm`) on `cloud`,
    /// installed in `dir`, on the runner whose home is `home` (the app data folder).
    pub(crate) fn new(home: &Path, dir: &Path, id: &str, runtime: Runtime, cloud: &str) -> Result<CloudRun> {
        let processes = SystemProcesses::for_host(|var| std::env::var(var).ok(), crate::platform::home_dir().as_deref());
        let config = RunnerConfig::new(home).with_processes(Arc::new(processes));
        let manifest = lab::terraform(dir, runtime, cloud).0.join("run.json");
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(errors::from_runner)?;
        Ok(CloudRun { run: OpenedRun::new(config, manifest, name)?, cloud: cloud.to_owned() })
    }

    /// Refuses a change while the account's sign-in has run out, before anything runs: the
    /// cloud's CLI asked for what its Terraform provider signs in with ([`sign_in_question`]),
    /// with the account's environment. Any other failure of the question (no such CLI here, the
    /// network) is left to the change itself, which says what is wrong.
    pub(crate) async fn signed_in(&self, place: &CloudPlace) -> Result<()> {
        let Some((program, args)) = sign_in_question(&self.cloud) else { return Ok(()) };
        let mut env = place.tf_env.clone();
        if let Some(region) = place.region.as_ref().filter(|_| self.cloud == "aws") {
            env.push(("AWS_REGION".to_owned(), region.clone()));
        }
        match crate::platform::exec::run_env_timed(program, args, None, &env, SIGN_IN_WAIT).await {
            Err(Error::CommandFailed { stderr, .. }) => errors::expired_sign_in(&stderr, Some(&self.cloud)).map_or(Ok(()), Err),
            _ => Ok(()),
        }
    }

    /// Brings the lab up in the account `place` from its generated module, and says where to
    /// start. `env` is the launch's: the inputs the lab's slots take.
    pub(crate) async fn up(&self, place: &CloudPlace, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        self.signed_in(place).await?;
        super::up_with(&self.running()?, &place.options(env), log).await
    }

    /// Brings the running lab up to date with its regenerated module (a machine added to the lab
    /// or taken out), with the account's credentials (`place`'s); the machines that run keep what
    /// they were given.
    pub(crate) async fn update(&self, place: &CloudPlace, log: &mut impl FnMut(String)) -> Result<()> {
        self.signed_in(place).await?;
        self.update_with(&place.options(&[]), log).await
    }

    /// Takes `machine` out of the running lab ahead of its removal from the module, with the
    /// account's credentials (`place`'s).
    pub(crate) async fn retire(&self, place: &CloudPlace, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.signed_in(place).await?;
        self.retire_with(machine, &place.options(&[]), log).await
    }

    /// Removes everything the run created in the account, with the parameters it was applied with
    /// (the connection is `place`'s, the account's as it is now).
    pub(crate) async fn down(&self, place: &CloudPlace, log: &mut impl FnMut(String)) -> Result<()> {
        // A run whose module was never generated created nothing in the account: a clean no-op,
        // as the other runner paths tolerate (without this, tearing down a lab that was never
        // deployed errors on the missing run.json).
        if !self.manifest.is_file() {
            log("Nothing to remove: the run has no files.".to_owned());
            return Ok(());
        }
        self.signed_in(place).await?;
        let running = self.running()?;
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, running.down(&place.options(&[]), &sink)).await.map_err(errors::from_runner)
    }

    /// The lab as its page shows it (place `Cloud`); not running before its module was ever
    /// generated. The caller fills the account's name and the cloud's id (`status::on_server`).
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        if !self.manifest.is_file() {
            return Ok(LabStatus { place: Some(Place::Cloud), ..LabStatus::default() });
        }
        let running = self.running()?;
        let status = running.status().await.map_err(errors::from_runner)?;
        Ok(LabStatus { place: Some(Place::Cloud), ..view::lab_status(running.manifest(), &status, None) })
    }

    /// Runs the lab's checks over SSH; not available when the lab declares none, or before its
    /// module was ever generated (nothing is deployed to check).
    pub(crate) async fn check(&self, place: &CloudPlace) -> Result<Check> {
        if !self.manifest.is_file() {
            return Ok(Check::default());
        }
        super::checks_of(&self.running()?, &place.options(&[])).await
    }
}

/// The account's credentials as the run's connection secrets, for a destroy (a value no manifest
/// slot takes is never delivered).
fn connection_secrets(tf_env: &[(String, String)]) -> Secrets {
    let mut secrets = Secrets::new();
    for (k, v) in tf_env.iter().filter(|(k, _)| !PARAM_ENV.contains(&k.as_str())) {
        secrets.insert(k.clone(), SecretValue::new(v.clone()));
    }
    secrets
}

/// Destroys every cloud run the runner records as past its expiry (the auto-stop), with the
/// account's credentials resolved by `account` (a recorded account id to its Terraform
/// environment). The runner replays the parameters each run was applied with. Each destroy runs
/// under the lab's per-lab lock (`lock`, `None` while another operation holds it): a user Stop or
/// install of the same lab must not run a second Terraform operation over the same state, which
/// could corrupt `terraform.tfstate`. Returns the runs it couldn't destroy, with why, so the
/// caller can surface and retry them; a run whose account can't be resolved, or whose lab is
/// busy, is left for a later sweep.
///
/// # Errors
///
/// Never returns an error itself: a sweep that can't even list the runs yields that as one entry.
pub(crate) async fn reap_expired<F, L, G>(home: &Path, now: SystemTime, account: F, lock: L) -> Vec<(String, Error)>
where
    F: Fn(&str) -> Option<Vec<(String, String)>>,
    L: Fn(&str) -> Option<G>,
{
    let runner = match Runner::new(super::host_config(home, None)) {
        Ok(runner) => runner,
        Err(e) => return vec![("runner".to_owned(), errors::from_runner(e))],
    };
    let expired = match runner.expired(now) {
        Ok(records) => records,
        Err(e) => return vec![("runner".to_owned(), errors::from_runner(e))],
    };
    let mut failed = Vec::new();
    for record in expired.into_iter().filter(|r| r.cloud.is_some()) {
        let name = record.name.to_string();
        // Serialize with the launcher's own operations on this lab (`lock_lab`): busy means
        // another operation holds it, so leave it for a later sweep rather than destroy over it
        // (not a failure). Held across the destroy below.
        let id = name.strip_prefix("cyberctf-").unwrap_or(&name);
        let Some(_held) = lock(id) else {
            log::debug!("auto-stop: {name} is busy (an operation holds its lab lock); left for a later sweep");
            continue;
        };
        let Some(host) = record.host_label.as_deref() else {
            failed.push((name, Error::Invalid("no account is recorded for it; remove its resources in your cloud console".to_owned())));
            continue;
        };
        // The account removed or unreadable: left for a later sweep rather than reported as a
        // destroy failure (it may come back). Said in the log, so a lab billing on is not silent.
        let Some(tf_env) = account(host) else {
            log::warn!("auto-stop: {name} is past its stop time, but its account {host} can't be used now; left for a later sweep");
            continue;
        };
        let open = runner.open(&record.manifest, Some(record.name.clone())).map(|r| r.with_connection(connection_secrets(&tf_env)));
        let sink = progress::silent();
        let outcome = match open {
            Ok(run) => run.down(&RunOptions::new(), &sink).await.map_err(errors::from_runner),
            Err(e) => Err(errors::from_runner(e)),
        };
        if let Err(e) = outcome {
            failed.push((name, e));
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

    /// An AWS sign-in that ran out stops a change before anything runs, said as signing in again;
    /// a valid one, or another cloud, lets it go on.
    #[tokio::test]
    async fn a_change_on_aws_waits_for_a_signed_in_account() {
        let home = std::env::temp_dir().join(format!("cyberctf-signin-{}", rand::random::<u32>()));
        let place = CloudPlace { region: Some("eu-west-3".into()), tf_env: vec![("AWS_PROFILE".into(), "labs".into())], ..CloudPlace::default() };
        let aws = CloudRun::new(&home, &home.join("lab"), "t", Runtime::Docker, "aws").unwrap();
        let tools = Tools::rules(&[("sts get-caller-identity", fail("Error: login session has expired, please reauthenticate"))]);
        let err = aws.down(&place, &mut |_| ()).await;
        assert!(err.is_ok(), "no files: nothing to remove, nothing asked");
        let err = aws.update(&place, &mut |_| ()).await.unwrap_err().to_string();
        assert!(err.starts_with("Your sign-in to AWS has expired") && err.contains("`aws login`"), "{err}");
        assert_eq!(tools.calls().len(), 1, "only the question ran: {:?}", tools.calls());
        drop(tools);
        let signed = Tools::rules(&[("sts get-caller-identity", ok("arn:aws:iam::1:user/me\n"))]);
        assert!(aws.signed_in(&place).await.is_ok());
        drop(signed);
        // Every cloud whose sign-in runs out is asked its own question; a token cloud, none.
        for (cloud, question, said) in [
            ("azure", "account get-access-token", "ERROR: AADSTS700082: The refresh token has expired. Please run 'az login'"),
            ("gcp", "application-default print-access-token", "Reauthentication failed. cannot prompt during non-interactive execution."),
            ("oci", "iam region list", "ServiceError: NotAuthenticated"),
        ] {
            let run = CloudRun::new(&home, &home.join("lab"), "t", Runtime::Docker, cloud).unwrap();
            let tools = Tools::rules(&[(question, fail(said))]);
            let err = run.update(&place, &mut |_| ()).await.unwrap_err().to_string();
            assert!(err.starts_with("Your sign-in to") && tools.calls().len() == 1, "{cloud}: {err} {:?}", tools.calls());
        }
        let linode = CloudRun::new(&home, &home.join("lab"), "t", Runtime::Docker, "linode").unwrap();
        let none = Tools::rules(&[]);
        assert!(linode.signed_in(&place).await.is_ok());
        assert!(none.calls().is_empty(), "no question for a token cloud");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A cloud run the runner lists as expired, as it records one: `cyberctf-<lab>`, started on
    /// account `host`, its stop time long past. Its manifest is not there (the lab's files gone).
    fn expired_record(home: &Path, lab: &str, host: &str) {
        let name = format!("cyberctf-{lab}");
        let dir = home.join("runs").join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        let record = serde_json::json!({
            "version": 0, "name": name, "manifest": home.join(lab).join("run.json"), "manifest_sha256": "ab".repeat(32),
            "target": "cloud-docker", "driver": "terraform", "cloud": "aws", "host_label": host, "state": "ready", "params": {}, "expires_at": 1
        });
        std::fs::write(dir.join("record.json"), record.to_string()).unwrap();
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
        let failed = reap_expired(&home, SystemTime::now(), account, |_| Some(())).await;
        assert_eq!(asked.into_inner().unwrap(), ["gone1", "aws1"], "every expired run's account is looked up");
        let names: Vec<&str> = failed.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["cyberctf-web"], "{failed:?}");
        assert!(home.join("runs").join("cyberctf-orphan").join("record.json").is_file(), "the skipped run keeps its record");
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
        let failed = reap_expired(&home, SystemTime::now(), account, lock).await;
        assert_eq!(asked.into_inner().unwrap(), ["aws1"], "only the free lab's account is used, never the busy one's");
        let names: Vec<&str> = failed.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["cyberctf-web"], "the free lab is swept (its files are gone, so its destroy fails); the busy one is not touched: {failed:?}");
        assert!(home.join("runs").join("cyberctf-busy").join("record.json").is_file(), "the busy run keeps its record (never destroyed)");
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
        }
    }

    #[test]
    fn credentials_are_connection_secrets_while_region_and_profile_are_parameters() {
        let opts = place().options(&[("FLAG".to_owned(), "flag{x}".to_owned())]);
        // The account's credentials and the launch's input slots travel as secrets…
        assert_eq!(opts.secrets.get("AWS_ACCESS_KEY_ID").map(SecretValue::expose), Some("AKIA"));
        assert_eq!(opts.secrets.get("AWS_SECRET_ACCESS_KEY").map(SecretValue::expose), Some("sk"));
        assert_eq!(opts.secrets.get("FLAG").map(SecretValue::expose), Some("flag{x}"));
        // …the region and profile do not (they are parameters).
        assert!(opts.secrets.get("AWS_PROFILE").is_none() && opts.secrets.get("AWS_REGION").is_none());
        assert_eq!(opts.params.get("region").map(String::as_str), Some("us-east-1"));
        assert_eq!(opts.params.get("allowed_cidr").map(String::as_str), Some("203.0.113.7/32"));
        assert_eq!(opts.params.get("AWS_PROFILE").map(String::as_str), Some("labs"));
        assert_eq!(opts.host_label.as_deref(), Some("aws1"));
        assert!(opts.lifetime.is_some(), "the account's auto-stop is the run's expiry");
    }

    #[test]
    fn connection_secrets_for_a_destroy_are_the_credentials_without_the_parameters() {
        // The same split a destroy relies on: every account var is a connection secret except the
        // region/profile parameters, which the runner replays from the applied params instead.
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
        let secrets = connection_secrets(&tf_env);
        assert_eq!(secrets.get("AWS_ACCESS_KEY_ID").map(SecretValue::expose), Some("AKIA"));
        assert_eq!(secrets.get("AWS_SECRET_ACCESS_KEY").map(SecretValue::expose), Some("sk"));
        assert!(secrets.get("AWS_PROFILE").is_none(), "the profile is a parameter, not a credential");
        assert!(secrets.get("AWS_REGION").is_none() && secrets.get("AWS_DEFAULT_REGION").is_none(), "the region is a parameter");
    }

    #[test]
    fn a_destroy_sets_no_firewall_or_expiry_and_leaves_the_runner_to_replay_the_rest() {
        let place = CloudPlace { region: Some("eu-west-3".to_owned()), tf_env: vec![("AWS_ACCESS_KEY_ID".to_owned(), "k".to_owned())], ..Default::default() };
        let opts = place.options(&[]);
        assert!(!opts.params.contains_key("allowed_cidr"), "no firewall value at destroy");
        assert!(opts.lifetime.is_none());
        assert_eq!(opts.params.get("region").map(String::as_str), Some("eu-west-3"));
        assert_eq!(opts.secrets.get("AWS_ACCESS_KEY_ID").map(SecretValue::expose), Some("k"));
    }
}
