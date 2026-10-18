//! The launcher on the Isothrum runner: a container lab on this machine's Docker, or a VM lab on
//! this machine's hypervisor (Vagrant, every local provider), runs from the run manifest Isoloom
//! writes beside its files (`.isoloom[-<n>]/docker/run.json`, `.isoloom[-<n>]/vagrant/run.json`),
//! through the runner's `Running` operations, instead of the launcher's own `docker` and `vm` modules.
//! Which path runs a lab is the player's choice in Settings > Labs ([`chosen`]; the developer
//! override `CYBERCTF_RUNNER` wins): with the launcher's, nothing here runs.
//!
//! What stays the launcher's: the run's name (`cyberctf-<lab id>`, the Compose project the
//! launcher always used), the runner's home (the app data folder, so runs are kept under its
//! `runs/`), the attack box (the lab's on-demand access machine, hot-added into the run through
//! the runner — [`LabRun::attach`] — never run by the launcher), the words the player reads (the
//! runner's events in `progress`, its error codes in `errors`) and the lab page's shapes (`view`).
//! An operation that
//! outlives the app runs in the launcher's own deploy worker, which calls the runner in-process;
//! stopping that worker drops the operation, which ends what it started.

mod attack_box;
mod cloud;
mod errors;
mod import;
mod progress;
mod server;
mod view;

pub(crate) use attack_box::AttackStatus;
pub(crate) use cloud::{CloudPlace, CloudRun, reap_expired as reap_expired_clouds};
pub(crate) use import::{Outcome, import_all, runner as import_runner};
pub(crate) use server::{ServerPlace, ServerRun};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use isothrum_runner::{Occupied, RunName, RunOptions, Runner, RunnerConfig, Running, SecretValue, Secrets, ShellCommand, SystemProcesses};

use super::docker::{Check, PORTS_ENV};
use super::{LabStatus, Park, Place, Runtime, lab, paths};
use crate::error::Result;

/// The ESXi host's settings the vagrant target's manifest takes as params.
const ESXI_PARAMS: [&str; 5] = ["ESXI_HOSTNAME", "ESXI_HOSTPORT", "ESXI_USERNAME", "ESXI_DATASTORE", "ESXI_VIRTUAL_NETWORK"];

/// The ESXi host's password: the manifest's connection slot.
const ESXI_PASSWORD: &str = "ESXI_PASSWORD";

/// Where a VM lab on an ESXi server runs (Vagrant's `vmware_esxi`): the server's id, so an
/// operation asked for elsewhere is refused; its settings as the manifest's params; its password,
/// given to every operation of the run (status and park too). The runner relays the host's SSH
/// and HTTPS itself where this machine can't reach it (macOS).
#[derive(Clone)]
pub(crate) struct EsxiPlace {
    /// The server's id.
    pub(crate) label: String,
    /// The host's settings, by the manifest's param names.
    pub(crate) params: Vec<(String, String)>,
    password: SecretValue,
}

impl std::fmt::Debug for EsxiPlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EsxiPlace").field("label", &self.label).field("params", &self.params).finish_non_exhaustive()
    }
}

impl EsxiPlace {
    /// Server `label`'s place, from the environment the launcher keeps for its labs
    /// (`lab::esxi_env`'s names): the settings the manifest takes, and the password.
    pub(crate) fn from_env(label: &str, env: &[(String, String)]) -> EsxiPlace {
        let value = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        EsxiPlace {
            label: label.to_owned(),
            params: ESXI_PARAMS.iter().filter_map(|name| value(name).map(|v| ((*name).to_owned(), v))).collect(),
            password: SecretValue::new(value(ESXI_PASSWORD).unwrap_or_default()),
        }
    }

    /// The run's connection: the host's password.
    pub(crate) fn connection(&self) -> Secrets {
        let mut secrets = Secrets::new();
        secrets.insert(ESXI_PASSWORD, self.password.clone());
        secrets
    }
}

/// What a run is: a container lab on Docker, a container lab in one local VM (Isoloom's
/// `docker-vm`), or a VM lab on a local hypervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Docker,
    DockerVm,
    Vm,
}

/// One lab on this machine, run by the runner.
pub(crate) struct LabRun {
    runner: Runner,
    dir: PathBuf,
    name: RunName,
    kind: Kind,
    /// The ESXi server a VM lab runs on, if not on this machine.
    esxi: Option<EsxiPlace>,
}

impl LabRun {
    /// Container lab `id`, installed in `dir`, on the runner whose home is `home` (the app data
    /// folder), with this host's tools. The learner's attack box is the lab's on-demand access
    /// machine, hot-added into the run on demand (see [`LabRun::attach`]).
    pub(crate) fn docker(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::with(host_config(home), dir, id, Kind::Docker)
    }

    /// VM lab `id`, installed in `dir`, on the runner whose home is `home`, with this host's tools.
    pub(crate) fn vm(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::with(host_config(home), dir, id, Kind::Vm)
    }

    /// Container lab `id` in one local VM (Isoloom's `docker-vm`: Docker Compose inside a VM),
    /// installed in `dir`, on the runner whose home is `home`. Its on-demand attack box is a
    /// Compose service inside the VM, beside the containers, brought up by the runner.
    pub(crate) fn docker_in_vm(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::with(host_config(home), dir, id, Kind::DockerVm)
    }

    /// VM lab `id`, installed in `dir`, on the ESXi server `place`, through the runner whose home
    /// is `home`.
    pub(crate) fn on_esxi(home: &Path, dir: &Path, id: &str, runtime: Runtime, place: EsxiPlace) -> Result<LabRun> {
        let kind = if runtime == Runtime::Docker { Kind::DockerVm } else { Kind::Vm };
        Ok(LabRun::with(host_config(home), dir, id, kind)?.on(place))
    }

    /// The run on the ESXi server `place`.
    fn on(self, place: EsxiPlace) -> LabRun {
        LabRun { esxi: Some(place), ..self }
    }

    /// A run on a runner built from `config` (tests script its tools).
    fn with(config: RunnerConfig, dir: &Path, id: &str, kind: Kind) -> Result<LabRun> {
        let name = RunName::new(&super::docker::project(id)).map_err(errors::from_runner)?;
        let runner = Runner::new(config).map_err(errors::from_runner)?;
        Ok(LabRun { runner, dir: dir.to_path_buf(), name, kind, esxi: None })
    }

    /// The lab's manifest file.
    fn manifest(&self) -> PathBuf {
        match self.kind {
            Kind::Docker => lab::docker_manifest(&self.dir),
            Kind::DockerVm => lab::vagrant_dir(&self.dir, Runtime::Docker).join("run.json"),
            Kind::Vm => lab::vagrant_dir(&self.dir, Runtime::Vm).join("run.json"),
        }
    }

    /// The lab's manifest, opened under the run's name.
    fn lab(&self) -> Result<Running<'_>> {
        let lab = self.runner.open(&self.manifest(), Some(self.name.clone())).map_err(errors::from_runner)?;
        Ok(match &self.esxi {
            Some(place) => lab.with_connection(place.connection()),
            None => lab,
        })
    }

    /// `opts` for this run's place: on an ESXi server, its id and its settings.
    fn placed(&self, opts: RunOptions) -> RunOptions {
        match &self.esxi {
            Some(place) => opts.with_host_label(place.label.clone()).with_params(place.params.iter().cloned()),
            None => opts,
        }
    }

    /// Brings the lab up from its generated files (a parked run is resumed as it was), and says
    /// where to start: the lab's message, its host ports filled in. `env` is the launch's: the
    /// inputs the lab's slots take, and [`PORTS_ENV`]; a VM lab runs on `provider`.
    pub(crate) async fn up(&self, env: &[(String, String)], provider: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        up_with(&self.lab()?, &self.placed(start_options(env, provider)), log).await
    }

    /// Removes everything the run created: containers, networks, volumes, its pinned ports; or
    /// its VMs and Vagrant's record of them.
    pub(crate) async fn down(&self, log: &mut impl FnMut(String)) -> Result<()> {
        // A run whose manifest was never written (a start that failed before generation) created
        // nothing, so there is nothing to remove: a clean no-op, as the launcher's own Docker path
        // tolerates "no resource to remove". Without this, tearing down a failed launch errors on
        // the missing run.json.
        if !self.manifest().exists() {
            log("Nothing to remove: the run has no files.".into());
            return Ok(());
        }
        let lab = self.lab()?;
        let opts = self.placed(RunOptions::new());
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.down(&opts, &sink)).await.map_err(errors::from_runner)
    }

    /// Parks the lab, keeping what it created: Docker only stops containers, so both of the
    /// launcher's park modes are the manifest's `shutdown` there; VMs are suspended (`Pause`) or
    /// powered off (`Shutdown`).
    pub(crate) async fn park(&self, mode: Park, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let mode = match (self.kind, mode) {
            // VMs (a VM lab, or the VM a container lab runs in) suspend on Pause; Docker only
            // stops its containers, so both of its park modes are the manifest's `shutdown`.
            (Kind::Vm | Kind::DockerVm, Park::Pause) => isothrum_runner::Park::Pause,
            _ => isothrum_runner::Park::Shutdown,
        };
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.park(mode, &sink)).await.map_err(errors::from_runner)
    }

    /// Starts the parked lab again. When nothing is left of it, the launcher's parked mark goes
    /// too, so the lab page offers a fresh start instead of a resume that can't work.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let opts = self.placed(RunOptions::new());
        let (sink, lines) = progress::channel();
        match progress::follow(log, lines, lab.resume(&opts, &sink)).await {
            Ok(_) => Ok(()),
            Err(e) if e.code() == isothrum_runner::ErrorCode::NothingToResume => {
                paths::mark_parked(&self.dir, None)?;
                Err(errors::from_runner(e))
            }
            Err(e) => Err(errors::from_runner(e)),
        }
    }

    /// The lab as its page shows it; not running before its files were ever generated.
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        let place = if matches!(self.kind, Kind::Vm | Kind::DockerVm) { Place::LocalVm } else { Place::Container };
        if !self.manifest().is_file() {
            return Ok(LabStatus { place: Some(place), ..LabStatus::default() });
        }
        let lab = self.lab()?;
        let status = lab.status().await.map_err(errors::from_runner)?;
        Ok(LabStatus { place: Some(place), ..view::lab_status(lab.manifest(), &status, paths::parked(&self.dir)) })
    }

    /// Runs the lab's set-up again on its running machines (`machine`, or all): its provisioners,
    /// and its controller's play. The launch's values aren't kept, so none reaches it: an input's
    /// file keeps the value it was written with.
    pub(crate) async fn provision(&self, machine: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let opts = self.placed(RunOptions::new());
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.provision(machine, &opts, &sink)).await.map_err(errors::from_runner)
    }

    /// Runs the lab's checks; not available when it declares none (nothing runs then).
    pub(crate) async fn check(&self) -> Result<Check> {
        checks_of(&self.lab()?, &self.placed(RunOptions::new())).await
    }

    /// Hot-adds the learner's on-demand attack box onto a lab network (the only one, or `network`
    /// when the lab has several), with the box `image` the learner picked, through the runner.
    pub(crate) async fn attach(&self, network: Option<&str>, image: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.attach(None, network, image, &sink)).await.map_err(errors::from_runner)?;
        Ok(())
    }

    /// Takes the attack box back out of the running lab, leaving the rest of the lab up.
    pub(crate) async fn detach(&self, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.detach(None, &sink)).await.map_err(errors::from_runner)
    }

    /// The attack box's status: its on-demand access machine as the runner reports it, its address
    /// on `network` (or the only one), the lab's networks to pick from, and the shell into it.
    pub(crate) async fn attack_status(&self, network: Option<&str>) -> Result<AttackStatus> {
        if !self.manifest().is_file() {
            return Ok(AttackStatus::default());
        }
        let lab = self.lab()?;
        let status = lab.status().await.map_err(errors::from_runner)?;
        let shell = lab.shell_command(None).ok().map(|c| attack_box::display(&c)).unwrap_or_default();
        Ok(attack_box::status_of(&status, network, shell))
    }

    /// The command that opens a shell into the attack box: the runner builds it (it alone knows
    /// the Compose project and files, the Vagrant define), the launcher runs it in a terminal.
    pub(crate) fn shell_command(&self) -> Result<ShellCommand> {
        self.lab()?.shell_command(None).map_err(errors::from_runner)
    }
}

/// Brings `lab` up with `opts`, its events as `log`'s lines, then says where to start: its
/// message, filled.
async fn up_with(lab: &Running<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
    let (sink, lines) = progress::channel();
    let run = progress::follow(log, lines, lab.up(opts, &sink)).await.map_err(errors::from_runner)?;
    run.message.iter().flat_map(|m| m.lines()).for_each(|l| log(l.to_owned()));
    Ok(())
}

/// Runs `lab`'s checks; not available when it declares none (nothing runs then).
async fn checks_of(lab: &Running<'_>, opts: &RunOptions) -> Result<Check> {
    if lab.manifest().checks.runners.is_empty() {
        return Ok(view::no_checks());
    }
    let mut output = Vec::new();
    let (sink, lines) = progress::channel();
    let report = progress::follow(&mut |l| output.push(l), lines, lab.check(opts, &sink)).await.map_err(errors::from_runner)?;
    Ok(view::check(&report, &output))
}

/// A runner at `home` with this host's tools.
fn host_config(home: &Path) -> RunnerConfig {
    let processes = SystemProcesses::for_host(|var| std::env::var(var).ok(), crate::platform::home_dir().as_deref());
    RunnerConfig::new(home).with_processes(Arc::new(processes))
}

/// A start's options: the launch's values for the manifest's secret and input slots (a value no
/// slot takes is never delivered), fixed host ports when the player chose the lab's own, the
/// hypervisor a VM lab runs on, and a parked run resumed rather than rebuilt.
fn start_options(env: &[(String, String)], provider: Option<&str>) -> RunOptions {
    let mut secrets = Secrets::new();
    for (name, value) in env.iter().filter(|(k, _)| k != PORTS_ENV) {
        secrets.insert(name.clone(), SecretValue::new(value.clone()));
    }
    let mut opts = RunOptions::new().with_secrets(secrets).with_occupied(Occupied::ResumeParked);
    if let Some(p) = provider {
        opts = opts.with_provider(p);
    }
    if env.iter().any(|(k, v)| k == PORTS_ENV && v == "default") { opts.with_param("ISOLOOM_PUBLISH_FIXED", "1") } else { opts }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vm_tests;
