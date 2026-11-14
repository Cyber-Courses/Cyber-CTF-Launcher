//! A lab run the launcher opens from its generated Terraform module under its own name: a
//! server's (Proxmox) or a cloud account's. Both are changed (brought up to date, a machine
//! retired), read and consoled into the same way.

use std::path::{Path, PathBuf};

use isothrum_runner::{Format, RunName, RunOptions, Runner, RunnerConfig, Running};

use super::errors;
use crate::error::Result;
use crate::runtime::lab;

/// A lab's run on the runner, opened by its name from the module Isoloom generated for it.
pub(crate) struct OpenedRun {
    pub(super) runner: Runner,
    /// The lab's folder.
    pub(super) dir: PathBuf,
    /// The Isoloom target the run's files are generated for; none for a helper VM an earlier
    /// version left on a Proxmox server, whose files Isoloom no longer generates.
    target: Option<lab::Target>,
    /// The run's Terraform module.
    pub(super) project: PathBuf,
    pub(super) name: RunName,
}

impl OpenedRun {
    /// The run named `name` of the lab in `dir`, whose Terraform module `project` is generated
    /// for `target` (none: a helper VM's, see [`OpenedRun::target`]), on a runner set up with
    /// `config`.
    pub(super) fn new(config: RunnerConfig, dir: &Path, target: Option<lab::Target>, project: PathBuf, name: RunName) -> Result<Self> {
        Ok(OpenedRun { runner: Runner::new(config).map_err(errors::from_runner)?, dir: dir.to_path_buf(), target, project, name })
    }

    /// Whether the module was generated (a start that failed before generation made none).
    pub(crate) fn has_project(&self) -> bool {
        std::fs::read_dir(&self.project).is_ok_and(|mut entries| entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "tf"))))
    }

    /// The Isoloom target the run's files are generated for; none for a run an earlier version
    /// left in a helper VM, which is only read, stopped and removed.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        self.target
    }

    /// The lab's module, opened under the run's name.
    pub(super) async fn running(&self) -> Result<Running<'_>> {
        self.runner.open_as(&self.project, Format::Terraform, Some(self.name.clone())).map_err(|e| match self.target {
            Some(_) => errors::from_runner(e),
            // A helper VM an earlier version left on a server: said what to remove if not opened.
            None => errors::of_helper_vm(e, self.name.as_str()),
        })
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<isothrum_runner::Status>> {
        if !self.has_project() {
            return Ok(None);
        }
        self.running().await?.status().await.map(Some).map_err(errors::from_runner)
    }

    /// Brings the running lab up to date with its regenerated files, with `opts` (what changing
    /// the run needs: the place's connection and credentials).
    pub(crate) async fn update_with(&self, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        super::update_of(&self.running().await?, opts, log).await
    }

    /// Takes `machine` out of the running lab ahead of its removal from the lab's files, with
    /// `opts`, as [`OpenedRun::update_with`].
    pub(crate) async fn retire_with(&self, machine: &str, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        super::retire_from(&self.running().await?, machine, opts, log).await
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given.
    pub(crate) async fn console_command(&self, machine: &str, user: Option<&str>) -> Result<isothrum_runner::ShellCommand> {
        self.running().await?.console_command(machine, user).map_err(errors::from_runner)
    }
}
