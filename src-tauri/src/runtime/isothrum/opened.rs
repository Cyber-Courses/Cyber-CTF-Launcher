//! A lab run the launcher opens from its generated manifest under its own name: a server's
//! (Proxmox) or a cloud account's. Both are changed (brought up to date, a machine retired), read
//! and consoled into the same way.

use std::path::PathBuf;

use isothrum_runner::{RunName, RunOptions, Runner, RunnerConfig, Running};

use super::errors;
use crate::error::Result;

pub(crate) struct OpenedRun {
    pub(super) runner: Runner,
    pub(super) manifest: PathBuf,
    pub(super) name: RunName,
}

impl OpenedRun {
    /// The run named `name` of the lab whose generated manifest is `manifest`, on a runner set up
    /// with `config`.
    pub(super) fn new(config: RunnerConfig, manifest: PathBuf, name: RunName) -> Result<Self> {
        Ok(OpenedRun { runner: Runner::new(config).map_err(errors::from_runner)?, manifest, name })
    }

    /// The lab's manifest, opened under the run's name.
    pub(super) fn running(&self) -> Result<Running<'_>> {
        self.runner.open(&self.manifest, Some(self.name.clone())).map_err(errors::from_runner)
    }

    /// The attack box as the lab page shows it when it is `machine`, a machine of the lab (the
    /// launcher added it to the lab): its status as the runner reports it, the console into it.
    pub(crate) async fn machine_box_status(&self, machine: &str, network: Option<&str>, vm: bool) -> Result<super::AttackStatus> {
        if !self.manifest.is_file() {
            return Ok(super::AttackStatus { vm, ..super::AttackStatus::default() });
        }
        super::machine_status_in(&self.running()?, machine, network, vm).await
    }

    /// Brings the running lab up to date with its regenerated files, with `opts` (what changing
    /// the run needs: the place's connection and credentials).
    pub(crate) async fn update_with(&self, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        super::update_of(&self.running()?, opts, log).await
    }

    /// Takes `machine` out of the running lab ahead of its removal from the lab's files, with
    /// `opts`, as [`OpenedRun::update_with`].
    pub(crate) async fn retire_with(&self, machine: &str, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        super::retire_from(&self.running()?, machine, opts, log).await
    }

    /// The command that opens a console into `machine`, any machine of the lab.
    pub(crate) fn console_command(&self, machine: &str) -> Result<isothrum_runner::ShellCommand> {
        self.running()?.console_command(machine).map_err(errors::from_runner)
    }
}
