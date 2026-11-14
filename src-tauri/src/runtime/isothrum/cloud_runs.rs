//! A lab in the player's cloud account through the Isothrum runner, as VMs (`cloud-vm`) or as
//! its containers in a VM (`cloud-docker`): the Terraform module Isoloom writes
//! (`.isoloom[-<n>]/cloud-vm/<cloud>/`, `.isoloom[-<n>]/cloud-docker/<cloud>/`), run through the
//! runner's operations instead of the launcher's own Terraform path.
//!
//! What stays the launcher's (policy, never execution): the run's name (`cyberctf-<lab id>`), the
//! AWS budget guard, this machine's public IP for the firewall, the GCP labs project, and turning
//! the player's cloud account into the module's inputs. The runner keeps the Terraform state, the
//! run's record and its expiry.

use std::path::Path;
use std::time::{Duration, SystemTime};

use isothrum_runner::{Format, RunName, RunOptions, RunRecord, Runner, SecretValue, Secrets};

use super::opened::OpenedRun;
use super::{errors, progress, view};
use crate::error::{Code, Error, Result};
use crate::runtime::docker::Check;
use crate::runtime::paths::lab_dir_in;
use crate::runtime::server::sign_in::sign_in_question;
use crate::runtime::{LabStatus, Place, Runtime, lab};

/// The account environment names the launcher passes to the module as plain values (not
/// secrets): a chosen AWS CLI profile and the region. Everything else in the account's Terraform
/// environment is a credential, delivered as a secret.
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
    /// The account's Google Cloud project (its labs project), for a module of the lab's own.
    pub(crate) project: Option<String>,
    /// When the run stops itself (the account's auto-stop): the manifest's `expiry`.
    pub(crate) lifetime: Option<Duration>,
}

impl CloudPlace {
    /// Account `conn`'s place, for an operation on its run: the account's id, region and
    /// credentials.
    pub(crate) fn of(conn: &crate::runtime::server::Connection) -> CloudPlace {
        let project = conn.tf_vars.iter().find(|(k, _)| k == "project").map(|(_, v)| v.clone());
        CloudPlace { label: conn.id.clone(), region: conn.region.clone(), tf_env: conn.tf_env.clone(), project, ..CloudPlace::default() }
    }

    /// Account `conn`'s place for a launch: [`CloudPlace::of`], with the account's auto-stop as
    /// the run's expiry, and the firewall open to `allowed_ip` only, when the lab's module takes
    /// one (cloud services take none).
    pub(crate) fn launch(conn: &crate::runtime::server::Connection, allowed_ip: Option<&str>) -> CloudPlace {
        let hours = conn.auto_stop_hours.unwrap_or(0);
        CloudPlace {
            allowed_cidr: allowed_ip.map(|ip| format!("{ip}/32")),
            lifetime: (hours > 0).then(|| Duration::from_secs(u64::from(hours) * 3600)),
            ..CloudPlace::of(conn)
        }
    }

    /// The run's secrets: every credential in the account's environment, by its
    /// environment-variable name.
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

    /// The module's plain values: the region, the firewall CIDR, and a chosen AWS CLI profile.
    fn values(&self) -> Vec<(String, String)> {
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

    /// Where a module of the lab's own (cloud services) works, as its provider reads it from the
    /// environment: the account's AWS region, Azure subscription, Google Cloud project and region,
    /// each that the account has.
    fn placement(&self) -> Vec<(String, String)> {
        let env = |name: &str| self.tf_env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        let candidates = [
            ("AWS_REGION", env("AWS_REGION").or_else(|| env("AWS_DEFAULT_REGION")).or_else(|| self.region.clone())),
            ("ARM_SUBSCRIPTION_ID", env("ARM_SUBSCRIPTION_ID")),
            ("GOOGLE_PROJECT", self.project.clone()),
            ("GOOGLE_REGION", self.region.clone()),
        ];
        candidates.into_iter().filter_map(|(name, value)| Some((name.to_owned(), value?))).collect()
    }

    /// Every plain value the module gets from this place: [`CloudPlace::values`], then its
    /// placement.
    fn plain(&self) -> Vec<(String, String)> {
        self.values().into_iter().chain(self.placement()).collect()
    }

    /// The options of a start on this account, with the launch's values `env`.
    pub(crate) fn options(&self, env: &[(String, String)]) -> RunOptions {
        self.options_over(env, &[])
    }

    /// The options of an operation on this account's run, with the launch's values `env`, over the
    /// plain values the run was started with (`kept`): those win, so a stop or an update acts on
    /// the region and account the start did; this place adds only what they lack.
    fn options_over(&self, env: &[(String, String)], kept: &[(String, String)]) -> RunOptions {
        let added = self.plain().into_iter().filter(|(k, _)| !kept.iter().any(|(name, _)| name == k));
        let values = kept.iter().cloned().chain(added);
        let opts = values.fold(RunOptions::new(), |o, (k, v)| o.with_env(k, v));
        let opts = opts.with_secrets(self.secrets(env)).with_host_label(&self.label);
        match self.lifetime {
            Some(lifetime) => opts.with_lifetime(lifetime),
            None => opts,
        }
    }
}

/// How long the sign-in question may take.
const SIGN_IN_WAIT: Duration = Duration::from_secs(30);

/// One cloud lab (`cloud-vm` or `cloud-docker`), run by the runner.
pub(crate) struct CloudRun {
    run: OpenedRun,
    /// The app data folder the runner's home is in: where the launcher's store keeps the values
    /// the run was started with.
    data: std::path::PathBuf,
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
        let config = super::host_config(home, None);
        let (out, target) = lab::terraform(dir, runtime, cloud);
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(errors::from_runner)?;
        Ok(CloudRun { run: OpenedRun::new(config, dir, Some(target), out, name)?, data: data_of(home), cloud: cloud.to_owned() })
    }

    /// Refuses a change while the account's sign-in has run out, before anything runs: the
    /// cloud's CLI asked for what its Terraform provider signs in with ([`sign_in_question`], as
    /// the account's Test and the setup ask it), with the account's environment. Any other
    /// failure of the question (no such CLI here, the network) is left to the change itself,
    /// which says what is wrong.
    pub(crate) async fn signed_in(&self, place: &CloudPlace) -> Result<()> {
        let Some(cloud) = crate::runtime::server::terraform_provider(&self.cloud) else { return Ok(()) };
        let Some((program, args)) = sign_in_question(cloud) else { return Ok(()) };
        let mut env = place.tf_env.clone();
        if let Some(region) = place.region.as_ref().filter(|_| cloud == crate::runtime::providers::Provider::Aws) {
            env.push(("AWS_REGION".to_owned(), region.clone()));
        }
        match crate::platform::exec::run_env_timed(program, args, None, &env, SIGN_IN_WAIT).await {
            Err(Error::CommandFailed { stderr, .. }) => errors::expired_sign_in(&stderr, Some(cloud)).map_or(Ok(()), Err),
            _ => Ok(()),
        }
    }

    /// Brings the lab up in the account `place` from its generated module, and says where to
    /// start. `env` is the launch's: the inputs the lab's slots take.
    pub(crate) async fn up(&self, place: &CloudPlace, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        self.signed_in(place).await?;
        // Kept before anything runs: a run started with values no later stop has could not be
        // destroyed with them.
        let plain = place.plain();
        crate::store::open_in(&self.data)?.write(|tx| crate::store::run_values::replace(tx, self.name.as_str(), &plain))?;
        super::up_with(&self.running().await?, &place.options(env), log).await?;
        let message = self.target().and_then(|t| super::described::message(&self.dir, t));
        message.iter().flat_map(|m| m.lines()).for_each(|l| log(l.to_owned()));
        Ok(())
    }

    /// Brings the running lab up to date with its regenerated module (a machine added to the lab
    /// or taken out), with the account's credentials (`place`'s); the machines that run keep what
    /// they were given.
    pub(crate) async fn update(&self, place: &CloudPlace, log: &mut impl FnMut(String)) -> Result<()> {
        self.signed_in(place).await?;
        super::update_of(&self.running().await?, &place.options_over(&[], &self.kept()?), log).await
    }

    /// Takes `machine` out of the running lab ahead of its removal from the module, with the
    /// account's credentials (`place`'s).
    pub(crate) async fn retire(&self, place: &CloudPlace, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.signed_in(place).await?;
        super::retire_from(&self.running().await?, machine, &place.options_over(&[], &self.kept()?), log).await
    }

    /// Removes everything the run created in the account, with the parameters it was applied with
    /// (the connection is `place`'s, the account's as it is now).
    pub(crate) async fn down(&self, place: &CloudPlace, log: &mut impl FnMut(String)) -> Result<()> {
        // A run whose module was never generated created nothing in the account: a clean no-op,
        // as the other runner paths tolerate (without this, tearing down a lab that was never
        // deployed errors on the missing module).
        if !self.has_project() {
            log("Nothing to remove: the run has no files.".to_owned());
            return Ok(());
        }
        self.signed_in(place).await?;
        let running = self.running().await?;
        let (sink, lines) = progress::channel();
        let opts = place.options_over(&[], &self.kept()?);
        progress::follow(log, lines, running.down(&opts, &sink)).await.map_err(errors::from_runner)?;
        forget_values(&self.data, self.name.as_str());
        Ok(())
    }

    /// `machine`'s graphical console, as the runner reaches it in the account `place`.
    pub(crate) async fn screen(&self, place: &CloudPlace, machine: &str) -> Result<isothrum_runner::Console> {
        self.graphical_console(machine, &place.options_over(&[], &self.kept()?)).await
    }

    /// What opens `machine` in its cloud's own console, in the account `place`.
    pub(crate) async fn native(&self, place: &CloudPlace, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        self.native_console(machine, &place.options_over(&[], &self.kept()?)).await
    }

    /// The plain values the run was started with, as the launcher kept them (none for a run an
    /// earlier build started).
    fn kept(&self) -> Result<Vec<(String, String)>> {
        kept_values(&self.data, self.name.as_str())
    }

    /// The lab as its page shows it (place `Cloud`); not running before its module was ever
    /// generated. The caller fills the account's name and the cloud's id (`status::on_server`).
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        if !self.has_project() {
            return Ok(LabStatus { place: Some(Place::Cloud), ..LabStatus::default() });
        }
        let running = self.running().await?;
        let status = running.status().await.map_err(errors::from_runner)?;
        let form = if self.target() == Some(lab::Target::CloudDocker) { super::described::Form::Containers } else { super::described::Form::Vms };
        let described = super::described::Described::of(&self.dir, form)?;
        Ok(LabStatus { place: Some(Place::Cloud), ..view::lab_status(&described, &status, None) })
    }

    /// The lab's checks: not available in a cloud account yet (the runner runs no command in a
    /// Terraform module's VMs).
    pub(crate) fn check(&self, _place: &CloudPlace) -> Check {
        Check::default()
    }
}

/// The app data folder of the runner whose home is `home` (`<data>/isothrum`).
fn data_of(home: &Path) -> std::path::PathBuf {
    home.parent().unwrap_or(home).to_path_buf()
}

/// The plain values run `run` was started with, kept in the store of data folder `data`.
fn kept_values(data: &Path, run: &str) -> Result<Vec<(String, String)>> {
    crate::store::open_in(data)?.read(|conn| crate::store::run_values::of(conn, run))
}

/// Forgets the values of run `run`, gone; a store that can't be written is logged (a later start
/// replaces them).
fn forget_values(data: &Path, run: &str) {
    if let Err(e) = crate::store::open_in(data).and_then(|store| store.write(|tx| crate::store::run_values::forget(tx, run))) {
        log::warn!("{run}: the values it was started with couldn't be forgotten: {e}");
    }
}

/// A destroy's options: the plain values the run was started with (`kept`), the account's
/// environment `tf_env` (its credentials as secrets, its profile and region as plain values where
/// the kept ones don't say).
fn destroy_options(tf_env: &[(String, String)], kept: &[(String, String)]) -> RunOptions {
    let mut secrets = Secrets::new();
    let mut opts = kept.iter().cloned().fold(RunOptions::new(), |o, (k, v)| o.with_env(k, v));
    for (k, v) in tf_env {
        if !PARAM_ENV.contains(&k.as_str()) {
            secrets.insert(k.clone(), SecretValue::new(v.clone()));
        } else if !opts.env.contains_key(k) {
            opts = opts.with_env(k.clone(), v.clone());
        }
    }
    opts.with_secrets(secrets)
}

/// Destroys every cloud run the runner records as past its expiry (the auto-stop), with the
/// account's credentials resolved by `account` (a recorded account id to its Terraform
/// environment). Each destroy runs
/// under the lab's per-lab lock (`lock`, `None` while another operation holds it): a user Stop or
/// install of the same lab must not run a second Terraform operation over the same state, which
/// could corrupt `terraform.tfstate`. Returns the runs it couldn't destroy, with why, so the
/// caller can surface and retry them; a run whose account can't be resolved, or whose lab is
/// busy, is left for a later sweep. `Err` when the runs can't even be listed: no sweep happened
/// (not a failed run named for the runner, which a caller would report as a lab).
pub(crate) async fn reap_expired<F, L, G>(home: &Path, now: SystemTime, account: F, lock: L) -> Result<Vec<(String, Error)>>
where
    F: Fn(&str) -> Option<Vec<(String, String)>>,
    L: Fn(&str) -> Option<G>,
{
    let runner = Runner::new(super::host_config(home, None)).map_err(errors::from_runner)?;
    let expired = runner.expired(now).map_err(errors::from_runner)?;
    let mut failed = Vec::new();
    for record in expired.into_iter().filter(|r| r.cloud.is_some()) {
        let name = record.name.to_string();
        if let Err(e) = reap_one(&runner, home, &record, &account, &lock).await {
            failed.push((name, e));
        }
    }
    Ok(failed)
}

/// Destroys the expired cloud run `record`. Ok when it is gone, and when it is left for a later
/// sweep (its lab busy, its account unusable now); the error when its destroy failed.
async fn reap_one<F, L, G>(runner: &Runner, home: &Path, record: &RunRecord, account: &F, lock: &L) -> Result<()>
where
    F: Fn(&str) -> Option<Vec<(String, String)>>,
    L: Fn(&str) -> Option<G>,
{
    let name = record.name.to_string();
    // Serialize with the launcher's own operations on this lab (`lock_lab`): busy means
    // another operation holds it, so leave it for a later sweep rather than destroy over it
    // (not a failure). Held across the destroy below.
    let id = super::super::docker::lab_of_project(&name).unwrap_or(&name);
    let Some(_held) = lock(id) else {
        log::debug!("auto-stop: {name} is busy (an operation holds its lab lock); left for a later sweep");
        return Ok(());
    };
    let Some(host) = record.host_label.as_deref() else {
        return Err(Error::coded(Code::ReapNoHost));
    };
    // The account removed or unreadable: left for a later sweep rather than reported as a
    // destroy failure (it may come back). Said in the log, so a lab billing on is not silent.
    let Some(tf_env) = account(host) else {
        log::warn!("auto-stop: {name} is past its stop time, but its account {host} can't be used now; left for a later sweep");
        return Ok(());
    };
    // With its lab no longer installed, the destroy goes on with the module its run kept.
    if let Err(e) = lab_dir_in(home, id) {
        log::warn!("auto-stop: {name}'s lab isn't installed any more ({e}); destroying it with the module its run kept");
    }
    let kept = kept_values(&data_of(home), &name)?;
    let run = runner.open_as(&record.project, Format::Terraform, Some(record.name.clone())).map_err(errors::from_runner)?;
    run.down(&destroy_options(&tf_env, &kept), &progress::silent()).await.map_err(errors::from_runner)?;
    forget_values(&data_of(home), &name);
    Ok(())
}

#[cfg(test)]
mod tests;
