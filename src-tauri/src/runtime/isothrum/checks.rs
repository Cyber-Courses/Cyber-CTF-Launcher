//! A lab's checks through the runner: a Compose lab's check runners as one-off services, a VM
//! lab's through Isoloom's `checks` provisioner (in its controller, booted for them, or in the VM
//! a check stands on).

use isothrum_runner::RunOptions;

use super::super::docker::Check;
use super::super::lab;
use super::{Kind, LabRun, Result, described, remote, view};

impl LabRun {
    /// Runs the lab's checks; not available when it declares none (nothing runs then).
    pub(crate) async fn check(&self) -> Result<Check> {
        let runners = self.described()?.check_runners;
        if runners.is_empty() {
            return Ok(Check::default());
        }
        self.require_started()?;
        let (mut lines, mut all_ok) = (Vec::new(), true);
        for runner in &runners {
            let (said, ok) = if self.kind == Kind::Docker { self.service_checks(runner).await? } else { self.vm_checks(runner).await? };
            lines.extend(said.into_iter().map(|l| (runner.clone(), l)));
            all_ok &= ok;
        }
        Ok(view::check(&lines, all_ok))
    }

    /// The checks of Compose service `runner` (`isoloom-check[-<position>]`), run as a one-off:
    /// what it printed, and whether it passed.
    async fn service_checks(&self, runner: &str) -> Result<(Vec<String>, bool)> {
        let mut said = Vec::new();
        let ok = self.act(remote::Act::Exec(runner, &[]), &self.placed(RunOptions::new()), &mut |l| said.push(l)).await.map_err(remote::Failure::into_error)?;
        Ok((said, ok))
    }

    /// The checks of VM `vm`, through Isoloom's `checks` provisioner, which reads the check
    /// scripts from the host again before it runs them: what it printed (the operation's log),
    /// and whether it passed. A provisioner that fails is a check that failed when it printed any
    /// check's line; one that printed none never ran them, and its error is the answer.
    async fn vm_checks(&self, vm: &str) -> Result<(Vec<String>, bool)> {
        let mut said = Vec::new();
        if vm == lab::CONTROLLER {
            // Halted once it set the lab up: booted (not set up again) for its checks.
            self.act(remote::Act::Attach(vm), &self.placed(RunOptions::new()), &mut |l| said.push(l)).await.map_err(remote::Failure::into_error)?;
        }
        let act = remote::Act::Provision(Some(vm), Some(described::VAGRANT_CHECKS));
        match self.act(act, &self.placed(RunOptions::new()), &mut |l| said.push(l)).await {
            Ok(ok) => Ok((said, ok)),
            Err(_) if said.iter().any(|l| described::check_line(l).is_some()) => Ok((said, false)),
            Err(e) => Err(e.into_error()),
        }
    }
}
