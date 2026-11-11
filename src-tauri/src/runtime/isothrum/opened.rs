//! A lab run the launcher opens from its generated manifest under its own name: a server's
//! (Proxmox) or a cloud account's. Both are changed (brought up to date, a machine retired), read
//! and consoled into the same way.

use std::path::{Path, PathBuf};

use isothrum_runner::{RunName, RunOptions, Runner, RunnerConfig, Running};

use super::{errors, regenerate};
use crate::error::Result;
use crate::runtime::lab;

/// A lab's run on the runner, opened by its name from the manifest Isoloom generated for it.
pub(crate) struct OpenedRun {
    pub(super) runner: Runner,
    /// The lab's folder.
    dir: PathBuf,
    /// The Isoloom target the run's files are generated for.
    target: lab::Target,
    pub(super) manifest: PathBuf,
    pub(super) name: RunName,
}

impl OpenedRun {
    /// The run named `name` of the lab in `dir`, whose manifest `manifest` is generated for
    /// `target`, on a runner set up with `config`.
    pub(super) fn new(config: RunnerConfig, dir: &Path, target: lab::Target, manifest: PathBuf, name: RunName) -> Result<Self> {
        Ok(OpenedRun { runner: Runner::new(config).map_err(errors::from_runner)?, dir: dir.to_path_buf(), target, manifest, name })
    }

    /// The Isoloom target the run's files are generated for.
    pub(crate) fn target(&self) -> lab::Target {
        self.target
    }

    /// The lab's manifest, opened under the run's name: its files generated again first when an
    /// earlier version of the launcher made them ([`regenerate::current`]).
    pub(super) async fn running(&self) -> Result<Running<'_>> {
        regenerate::current(&self.manifest, &self.dir, self.target).await?;
        self.runner.open(&self.manifest, Some(self.name.clone())).map_err(errors::from_runner)
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<isothrum_runner::Status>> {
        if !self.manifest.is_file() {
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

    /// `machine`'s graphical console (its screen), as the runner reaches it with the place's
    /// `connection` (a server's API credentials; none for a cloud's, whose consoles need none).
    pub(crate) async fn graphical_console(&self, machine: &str, connection: isothrum_runner::Secrets) -> Result<isothrum_runner::Console> {
        self.running().await?.with_connection(connection).graphical_console(machine).await.map_err(errors::from_runner)
    }
}
