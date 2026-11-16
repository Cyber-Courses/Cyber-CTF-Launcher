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
use std::time::Duration;

use isothrum_runner::{Format, RunOptions, RunRecord, SecretValue, Secrets};

use super::opened::{Module, OpenedRun};
use super::view;
use crate::error::Result;
use crate::runtime::docker::Check;
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
    /// run's secrets) and any profile/region (delivered as parameters).
    pub(crate) tf_env: Vec<(String, String)>,
    /// The account's Google Cloud project (its labs project), for a module of the lab's own.
    pub(crate) project: Option<String>,
    /// When the run stops itself (the account's auto-stop): the run's expiry.
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

    /// The options of a later operation on this account's run (a stop, an update, a screen): the
    /// account's credentials only. The runner replays the plain values the start was given, so
    /// the operation acts on the region and account the start did.
    pub(crate) fn later_options(&self) -> RunOptions {
        RunOptions::new().with_secrets(self.secrets(&[])).with_host_label(&self.label)
    }

    /// Every plain value the module gets from this place: [`CloudPlace::values`], then its
    /// placement.
    fn plain(&self) -> Vec<(String, String)> {
        self.values().into_iter().chain(self.placement()).collect()
    }

    /// The options of a start on this account, with the launch's values `env`: the module's plain
    /// values (which the runner's record keeps and every later operation replays), the
    /// credentials and launch values as secrets.
    pub(crate) fn options(&self, env: &[(String, String)]) -> RunOptions {
        let opts = self.plain().into_iter().fold(RunOptions::new(), |o, (k, v)| o.with_env(k, v));
        let opts = opts.with_secrets(self.secrets(env)).with_host_label(&self.label);
        match self.lifetime {
            Some(lifetime) => opts.with_lifetime(lifetime),
            None => opts,
        }
    }
}

/// One cloud lab (`cloud-vm` or `cloud-docker`), run by the runner.
pub(crate) struct CloudRun {
    run: OpenedRun,
}

/// The opened run and what reads it (its status, console, update and retire) are
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
        let (out, target) = lab::terraform(dir, runtime, cloud);
        Ok(CloudRun { run: OpenedRun::new(home, dir, id, (out, Module::Generated(target)))? })
    }

    /// [`CloudRun::new`] on a runner set up with `config`, which this test process serves (a
    /// test's scripted tools).
    #[cfg(test)]
    pub(crate) fn new_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, runtime: Runtime, cloud: &str) -> Result<CloudRun> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        CloudRun::new(&home, dir, id, runtime, cloud)
    }

    /// Cloud-services lab `id`, installed in `dir`: its own module (`cloud/`), run as
    /// it is, on the runner whose home is `home`.
    #[cfg(feature = "cloud-services")]
    pub(crate) fn own(home: &Path, dir: &Path, id: &str) -> Result<CloudRun> {
        let module = crate::runtime::cloud_services::lab_file::module_dir(dir);
        Ok(CloudRun { run: OpenedRun::new(home, dir, id, (module, Module::Own))? })
    }

    /// [`CloudRun::own`] on a runner set up with `config`, which this test process serves (a
    /// test's scripted tools).
    #[cfg(all(test, feature = "cloud-services"))]
    pub(crate) fn own_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str) -> Result<CloudRun> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        CloudRun::own(&home, dir, id)
    }

    /// Brings the cloud-services lab up in the account `place` from its own module, with
    /// `values` (its lab file's, checked against the module), and says where to start.
    #[cfg(feature = "cloud-services")]
    pub(crate) async fn up_own(
        &self,
        place: &CloudPlace,
        values: &crate::runtime::cloud_services::values::ModuleValues,
        log: &mut impl FnMut(String),
    ) -> Result<()> {
        let opts = values.plain.iter().fold(place.options(&[]), |o, (k, v)| o.with_env(k.clone(), v.clone()));
        let opts = values.secret.iter().fold(opts, |mut o, (k, v)| {
            o.secrets.insert(k.clone(), SecretValue::new(v.clone()));
            o
        });
        self.act(super::remote::Act::Up, &opts, log).await?;
        if let Some(message) = self.own_status().await?.message {
            message.lines().for_each(|l| log(l.to_owned()));
        }
        Ok(())
    }

    /// Brings the lab up in the account `place` from its generated module, and says where to
    /// start. `env` is the launch's: the inputs the lab's slots take.
    pub(crate) async fn up(&self, place: &CloudPlace, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        // The key the lab's instances let in: a plain value the record keeps, so every later
        // operation replays it.
        let opts = self.key_values().await?.into_iter().fold(place.options(env), |o, (k, v)| o.with_env(k, v));
        self.act(super::remote::Act::Up, &opts, log).await?;
        super::described::log_message(&self.dir, self.target(), log);
        Ok(())
    }

    /// Brings the running lab up to date with its regenerated module (a machine added to the lab
    /// or taken out), with the account's credentials (`place`'s); the machines that run keep what
    /// they were given.
    pub(crate) async fn update(&self, place: &CloudPlace, log: &mut impl FnMut(String)) -> std::result::Result<(), crate::runtime::host::Failed> {
        self.update_with(&place.later_options(), |e| e, log).await
    }

    /// Takes `machine` out of the running lab ahead of its removal from the module, with the
    /// account's credentials (`place`'s).
    pub(crate) async fn retire(&self, place: &CloudPlace, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.act(super::remote::Act::Retire(machine), &place.later_options(), log).await
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
        self.act(super::remote::Act::Down, &place.later_options(), log).await
    }

    /// `machine`'s graphical console, as the runner reaches it in the account `place`.
    pub(crate) async fn screen(&self, place: &CloudPlace, machine: &str) -> Result<isothrum_client::HeldConsole> {
        self.graphical_console(machine, &place.later_options()).await
    }

    /// What opens `machine` in its cloud's own console, in the account `place`.
    pub(crate) async fn native(&self, place: &CloudPlace, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        self.native_console(machine, &place.later_options()).await
    }

    /// The lab as its page shows it (place `Cloud`); not running before its module was ever
    /// generated. The caller fills the account's name and the cloud's id (`status::on_server`).
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        if !self.has_project() {
            return Ok(LabStatus { place: Some(Place::Cloud), ..LabStatus::default() });
        }
        #[cfg(feature = "cloud-services")]
        if self.module() == Module::Own {
            return self.own_status().await;
        }
        let status = self.read().await?;
        let form = if self.target() == Some(lab::Target::CloudDocker) { super::described::Form::Containers } else { super::described::Form::Vms };
        let described = super::described::Described::of(&self.dir, form)?;
        Ok(LabStatus { place: Some(Place::Cloud), ..view::lab_status(&described, &status, None) })
    }

    /// The lab's checks: not available in a cloud account yet (the runner runs no command in a
    /// Terraform module's VMs).
    #[expect(clippy::unused_self, reason = "a run's checks are asked of the run, as on the other hosts; this one has none to run yet")]
    pub(crate) fn check(&self, _place: &CloudPlace) -> Check {
        Check::default()
    }
}

/// A destroy's options: the account's credentials from its environment `tf_env`, as secrets. The
/// runner replays the plain values the start was given (region, firewall CIDR, profile).
fn destroy_options(tf_env: &[(String, String)]) -> RunOptions {
    let mut secrets = Secrets::new();
    for (k, v) in tf_env.iter().filter(|(k, _)| !PARAM_ENV.contains(&k.as_str())) {
        secrets.insert(k.clone(), SecretValue::new(v.clone()));
    }
    RunOptions::new().with_secrets(secrets)
}

/// The cloud runs the home's runner service lists as past their expiry (the auto-stop) and that
/// may still hold something: the reaper's to destroy.
///
/// # Errors
///
/// No service can be reached, or it can't read its records.
pub(crate) async fn expired_clouds(home: &Path) -> Result<Vec<RunRecord>> {
    Ok(super::remote::expired(home).await?.into_iter().filter(|r| r.cloud.is_some()).collect())
}

/// Destroys the expired cloud run `record` through the home's runner service, with the account's
/// credentials from its environment `tf_env` (the runner replays the plain values its start was
/// given).
///
/// # Errors
///
/// The destroy failed, or no service can be reached.
pub(crate) async fn destroy_expired(home: &Path, record: &RunRecord, tf_env: &[(String, String)]) -> Result<()> {
    let served = super::remote::Served { home: home.to_path_buf(), name: record.name.clone(), project: record.project.clone(), format: Format::Terraform };
    served.operate(super::remote::Act::Down, &destroy_options(tf_env), &mut |_| {}).await.map(drop).map_err(super::remote::Failure::into_error)
}

#[cfg(test)]
mod tests;
