//! A cloud VM lab (`cloud-vm`, `cloud-docker`) in the player's cloud account through the Isothrum
//! runner: the run manifest Isoloom writes beside its Terraform module
//! (`.isoloom[-<n>]/cloud-vm/<cloud>/run.json`, `.isoloom[-<n>]/cloud-docker/<cloud>/run.json`),
//! run through the runner's operations instead of the launcher's own Terraform path.
//!
//! What stays the launcher's (policy, never execution): the run's name (`cyberctf-<lab id>`), the
//! AWS budget guard, this machine's public IP for the firewall, the GCP labs project, and turning
//! the player's cloud account into the module's inputs. The runner keeps the Terraform state, the
//! run's record and the expiry a destroy replays; it reads the module's `ssh_user` output and
//! waits on the project delivery, so a cloud lab now actually comes up, is sealed and checked.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use isothrum_runner::{RunName, RunOptions, Runner, RunnerConfig, SecretValue, Secrets, SystemProcesses};

use super::{errors, progress, view};
use crate::error::{Error, Result};
use crate::runtime::docker::Check;
use crate::runtime::{LabStatus, Place, Runtime, lab};

/// The account environment names the launcher passes to the module as **parameters** (not
/// secrets): a chosen AWS CLI profile and the region. Everything else in the account's Terraform
/// environment is a credential, delivered as a connection secret.
const PARAM_ENV: [&str; 3] = ["AWS_PROFILE", "AWS_REGION", "AWS_DEFAULT_REGION"];

/// Where a cloud VM lab runs, as the launcher resolved it from the player's account.
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
    fn options(&self, env: &[(String, String)]) -> RunOptions {
        let opts = RunOptions::new().with_secrets(self.secrets(env)).with_params(self.params()).with_host_label(&self.label);
        match self.lifetime {
            Some(lifetime) => opts.with_lifetime(lifetime),
            None => opts,
        }
    }
}

/// One cloud VM lab, run by the runner.
pub(crate) struct CloudRun {
    runner: Runner,
    manifest: PathBuf,
    name: RunName,
}

impl CloudRun {
    /// Lab `id` of `runtime` (a container lab is `cloud-docker`, a VM lab `cloud-vm`) on `cloud`,
    /// installed in `dir`, on the runner whose home is `home` (the app data folder).
    pub(crate) fn new(home: &Path, dir: &Path, id: &str, runtime: Runtime, cloud: &str) -> Result<CloudRun> {
        let processes = SystemProcesses::for_host(|var| std::env::var(var).ok(), crate::platform::home_dir().as_deref());
        let config = RunnerConfig::new(home).with_processes(Arc::new(processes));
        let manifest = lab::terraform(dir, runtime, cloud)?.0.join("run.json");
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(errors::from_runner)?;
        Ok(CloudRun { runner: Runner::new(config).map_err(errors::from_runner)?, manifest, name })
    }

    /// The lab's manifest, opened under the run's name.
    fn running(&self) -> Result<isothrum_runner::Running<'_>> {
        self.runner.open(&self.manifest, Some(self.name.clone())).map_err(errors::from_runner)
    }

    /// Brings the lab up in the account `place` from its generated module, and says where to
    /// start. `env` is the launch's: the inputs the lab's slots take.
    pub(crate) async fn up(&self, place: &CloudPlace, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        super::up_with(&self.running()?, &place.options(env), log).await
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
            return Ok(view::no_checks());
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
/// environment). The runner replays the parameters each run was applied with. Returns the runs it
/// couldn't destroy, with why, so the caller can surface and retry them; a run whose account can't
/// be resolved is left for a later sweep.
///
/// # Errors
///
/// Never returns an error itself: a sweep that can't even list the runs yields that as one entry.
pub(crate) async fn reap_expired<F>(home: &Path, now: SystemTime, account: F) -> Vec<(String, Error)>
where
    F: Fn(&str) -> Option<Vec<(String, String)>>,
{
    let runner = match Runner::new(super::host_config(home)) {
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
        let Some(host) = record.host_label.as_deref() else {
            failed.push((name, Error::Invalid("no account is recorded for it; remove its resources in your cloud console".to_owned())));
            continue;
        };
        // The account removed or unreadable: left for a later sweep rather than reported as a
        // destroy failure (it may come back).
        let Some(tf_env) = account(host) else { continue };
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
