//! The launcher on the Isothrum runner: a container lab on this machine's Docker runs from the
//! run manifest Isoloom writes beside its Compose file (`.isoloom[-<n>]/docker/run.json`), through
//! the runner's `Lab` operations, instead of the launcher's own `docker` module. Phase 2 keeps it
//! behind a hidden developer switch (`CYBERCTF_RUNNER=isothrum`, see [`chosen`]): with it off,
//! nothing here runs and the `docker` module does what it always did.
//!
//! What stays the launcher's: the run's name (`cyberctf-<lab id>`, the Compose project the
//! launcher always used), the runner's home (the app data folder, so runs are kept under its
//! `runs/`), the attack box (the runner's hooks, `attack_box`), the words the player reads (the
//! runner's events in `progress`, its error codes in `errors`) and the lab page's shapes (`view`).
//! An operation that outlives the app runs in the launcher's own deploy worker, which calls the
//! runner in-process; stopping that worker drops the operation, which ends what it started.

mod attack_box;
mod errors;
mod progress;
mod server;
mod view;

pub(crate) use server::{ServerPlace, ServerRun};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use isothrum_runner::{Lab, Occupied, RunName, RunOptions, Runner, RunnerConfig, SecretValue, Secrets, SystemProcesses};

use super::docker::{Check, PORTS_ENV};
use super::{LabStatus, Place, lab, paths};
use crate::error::Result;

/// Whether this launcher runs container labs on this machine's Docker through the runner.
pub(crate) fn chosen() -> bool {
    crate::platform::config::isothrum_runner()
}

/// One container lab on this machine's Docker, run by the runner.
pub(crate) struct DockerRun {
    runner: Runner,
    dir: PathBuf,
    name: RunName,
}

impl DockerRun {
    /// Lab `id`, installed in `dir`, on the runner whose home is `home` (the app data folder),
    /// with this host's tools.
    pub(crate) fn new(home: &Path, dir: &Path, id: &str) -> Result<DockerRun> {
        let processes = SystemProcesses::for_host(|var| std::env::var(var).ok(), crate::platform::home_dir().as_deref());
        DockerRun::with(RunnerConfig::new(home).with_processes(Arc::new(processes)), dir, id)
    }

    /// [`DockerRun::new`] on a runner built from `config` (tests script its tools).
    fn with(config: RunnerConfig, dir: &Path, id: &str) -> Result<DockerRun> {
        let name = RunName::new(&super::docker::project(id)).map_err(errors::from_runner)?;
        let mut runner = Runner::new(config).map_err(errors::from_runner)?;
        runner.add_hooks(Arc::new(attack_box::AttackBox::new(id)));
        Ok(DockerRun { runner, dir: dir.to_path_buf(), name })
    }

    /// The lab's manifest, opened under the run's name.
    fn lab(&self) -> Result<Lab<'_>> {
        self.runner.open(&lab::docker_manifest(&self.dir), Some(self.name.clone())).map_err(errors::from_runner)
    }

    /// Brings the lab up from its generated files (a parked run is resumed as it was), and says
    /// where to start: the lab's message, its host ports filled in. `env` is the launch's: the
    /// inputs the lab's slots take, and [`PORTS_ENV`].
    pub(crate) async fn up(&self, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        up_with(&self.lab()?, &start_options(env), log).await
    }

    /// Removes everything the run created: containers, networks, volumes, its pinned ports.
    pub(crate) async fn down(&self, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let opts = RunOptions::new();
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.down(&opts, &sink)).await.map_err(errors::from_runner)
    }

    /// Stops the lab's containers, keeping them: Docker only stops containers, so both of the
    /// launcher's park modes are the manifest's `shutdown`.
    pub(crate) async fn park(&self, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.park(isothrum_runner::Park::Shutdown, &sink)).await.map_err(errors::from_runner)
    }

    /// Starts the parked containers again. When nothing is left of them, the launcher's parked
    /// mark goes too, so the lab page offers a fresh start instead of a resume that can't work.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let opts = RunOptions::new();
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
        if !lab::docker_manifest(&self.dir).is_file() {
            return Ok(LabStatus { place: Some(Place::Container), ..LabStatus::default() });
        }
        let lab = self.lab()?;
        let status = lab.status().await.map_err(errors::from_runner)?;
        Ok(view::lab_status(lab.manifest(), &status, paths::parked(&self.dir)))
    }

    /// Runs the lab's checks; not available when it declares none (nothing runs then).
    pub(crate) async fn check(&self) -> Result<Check> {
        checks_of(&self.lab()?).await
    }
}

/// Brings `lab` up with `opts`, its events as `log`'s lines, then says where to start: its
/// message, filled.
async fn up_with(lab: &Lab<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
    let (sink, lines) = progress::channel();
    let run = progress::follow(log, lines, lab.up(opts, &sink)).await.map_err(errors::from_runner)?;
    run.message.iter().flat_map(|m| m.lines()).for_each(|l| log(l.to_owned()));
    Ok(())
}

/// Runs `lab`'s checks; not available when it declares none (nothing runs then).
async fn checks_of(lab: &Lab<'_>) -> Result<Check> {
    if lab.manifest().checks.runners.is_empty() {
        return Ok(view::no_checks());
    }
    let mut output = Vec::new();
    let (sink, lines) = progress::channel();
    let report = progress::follow(&mut |l| output.push(l), lines, lab.check(&RunOptions::new(), &sink)).await.map_err(errors::from_runner)?;
    Ok(view::check(&report, &output))
}

/// A start's options: the launch's values for the manifest's secret and input slots (a value no
/// slot takes is never delivered), fixed host ports when the player chose the lab's own, and a
/// parked run resumed rather than rebuilt.
fn start_options(env: &[(String, String)]) -> RunOptions {
    let mut secrets = Secrets::new();
    for (name, value) in env.iter().filter(|(k, _)| k != PORTS_ENV) {
        secrets.insert(name.clone(), SecretValue::new(value.clone()));
    }
    let opts = RunOptions::new().with_secrets(secrets).with_occupied(Occupied::ResumeParked);
    if env.iter().any(|(k, v)| k == PORTS_ENV && v == "default") { opts.with_param("ISOLOOM_PUBLISH_FIXED", "1") } else { opts }
}

#[cfg(test)]
mod tests;
