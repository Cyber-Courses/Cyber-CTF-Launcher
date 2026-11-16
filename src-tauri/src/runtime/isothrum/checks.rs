//! A lab's checks through the runner: a Compose lab's check runners as one-off services, a VM
//! lab's through Isoloom's `checks` provisioner (in its controller, booted for them, or in the VM
//! a check stands on), and a Terraform module's VMs' (a server's) through the runner's `exec`
//! into each machine the module's `checks` output names.

use std::path::Path;

use isothrum_runner::RunOptions;

use super::super::docker::Check;
use super::super::lab;
use super::opened::OpenedRun;
use super::{Kind, LabRun, Result, described, remote, view};
use crate::error::Error;

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

/// One runner of a Terraform module's `checks` output: the position it runs from, the machine it
/// runs in, and what it runs there: its script (a path from the project folder, read here and
/// handed over: a server's), or its command (from the project copy in the machine: a cloud's).
#[derive(Debug, serde::Deserialize)]
struct ModuleCheck {
    position: String,
    machine: String,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

impl OpenedRun {
    /// Runs the lab's checks: each runner the module's `checks` output names, in its machine, by
    /// the runner (`exec`, an SSH along the module's route: through its router, and through the
    /// server's node first when only it reaches the lab), with `opts`. Not available when the
    /// module names none. A runner that couldn't run is a check that failed when it printed any
    /// check's line; one that printed none never ran them, and its error is the answer.
    pub(crate) async fn module_check(&self, opts: &RunOptions) -> Result<Check> {
        let status = self.read_with(opts).await?;
        let listed = status.outputs.iter().find(|o| o.name == "checks").and_then(|o| o.revealed());
        let checks: Vec<ModuleCheck> = listed.and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default();
        if checks.is_empty() {
            return Ok(Check::default());
        }
        let (mut lines, mut all_ok) = (Vec::new(), true);
        for check in &checks {
            let argv = ["sh".to_owned(), "-c".to_owned(), self.runner_code(check)?];
            let mut said = Vec::new();
            let ok = match self.run(remote::Act::Exec(&check.machine, &argv), opts, &mut |l| said.push(l)).await {
                Ok(ok) => ok,
                Err(_) if said.iter().any(|l| described::check_line(l).is_some()) => false,
                Err(e) => return Err(e.into_error()),
            };
            lines.extend(said.into_iter().map(|l| (check.position.clone(), l)));
            all_ok &= ok;
        }
        Ok(view::check(&lines, all_ok))
    }

    /// What runner `check` runs: its script, from the module's own `checks/` folder (the output
    /// names it from the project's root, which an instance's folder moves), else its command.
    fn runner_code(&self, check: &ModuleCheck) -> Result<String> {
        if let Some(script) = &check.script {
            let name = Path::new(script).file_name().ok_or_else(|| Error::Invalid(format!("the lab's check runner {script} names no file")))?;
            let path = self.project.join("checks").join(name);
            return std::fs::read_to_string(&path).map_err(|e| Error::Invalid(format!("the lab's check runner {} couldn't be read: {e}", path.display())));
        }
        check.command.clone().ok_or_else(|| Error::Invalid(format!("the lab's check runner at {} says nothing to run", check.position)))
    }
}
