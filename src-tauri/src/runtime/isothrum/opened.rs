//! A lab run the launcher opens from its generated manifest under its own name: a server's
//! (Proxmox) or a cloud account's. The attack box is added, taken out, read and shelled into the
//! same way on both: a container in the VM the lab's containers run in.

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

    /// Hot-adds the learner's attack box into the running lab: a container in the VM the lab
    /// runs its containers in, on `network` (the only one otherwise), from `image`. A VM lab
    /// there has no VM to put it in: the runner refuses.
    pub(crate) async fn attach(&self, network: Option<&str>, image: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        self.attach_with(&RunOptions::new(), network, image, log).await
    }

    /// [`OpenedRun::attach`] with `opts`: what changing the run needs (a cloud account's
    /// credentials, when its module makes the box an instance of its own).
    pub(crate) async fn attach_with(&self, opts: &RunOptions, network: Option<&str>, image: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let pick = isothrum_runner::BoxPick { machine: None, network, image };
        super::attach_into(&self.running()?, opts, pick, log).await
    }

    /// Takes the attack box back out of the running lab, leaving the rest of it up.
    pub(crate) async fn detach(&self, log: &mut impl FnMut(String)) -> Result<()> {
        self.detach_with(&RunOptions::new(), log).await
    }

    /// [`OpenedRun::detach`] with `opts`, as [`OpenedRun::attach_with`].
    pub(crate) async fn detach_with(&self, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        super::detach_from(&self.running()?, opts, log).await
    }

    /// The attack box as the lab page shows it (nothing before the lab's files exist).
    pub(crate) async fn attack_status(&self, network: Option<&str>) -> Result<super::AttackStatus> {
        if !self.manifest.is_file() {
            return Ok(super::AttackStatus::default());
        }
        super::attack_status_in(&self.running()?, network, false).await
    }

    /// The attack box as the lab page shows it when it is `machine`, a machine of the lab (the
    /// launcher added it to the lab): its status as the runner reports it, the console into it.
    pub(crate) async fn machine_box_status(&self, machine: &str, network: Option<&str>) -> Result<super::AttackStatus> {
        if !self.manifest.is_file() {
            return Ok(super::AttackStatus::default());
        }
        let running = self.running()?;
        let status = running.status().await.map_err(errors::from_runner)?;
        let shell = running.console_command(machine).ok().map(|c| super::attack_box::display(&c)).unwrap_or_default();
        Ok(super::attack_box::status_of(&status, network, shell, false))
    }

    /// The command that opens a console into `machine`, any machine of the lab.
    pub(crate) fn console_command(&self, machine: &str) -> Result<isothrum_runner::ShellCommand> {
        self.running()?.console_command(machine).map_err(errors::from_runner)
    }

    /// The command that opens a shell into the attack box: `ssh -t` to the lab's VM, then into
    /// the box, as the runner builds it.
    pub(crate) fn shell_command(&self) -> Result<isothrum_runner::ShellCommand> {
        self.running()?.shell_command(None).map_err(errors::from_runner)
    }
}
