//! A lab run the launcher opens from its generated Terraform module under its own name: a
//! server's (Proxmox) or a cloud account's. Both are changed (brought up to date, a machine
//! retired), read and consoled into the same way.

use std::path::{Path, PathBuf};

use isothrum_runner::{Format, RunName, RunOptions, Runner, RunnerConfig, Running};

use super::{errors, remote};
use crate::error::Result;
use crate::runtime::lab;

/// What a run's Terraform module is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Module {
    /// The one Isoloom generates for a target.
    Generated(lab::Target),
    /// The lab's own, hand-written (a cloud-services lab's `cloud/`).
    Own,
    /// A helper VM's an earlier version left on a Proxmox server, whose files Isoloom no longer
    /// generates: only read, stopped and removed.
    HelperVm,
}

/// A lab's run on the runner, opened by its name from its Terraform module.
pub(crate) struct OpenedRun {
    pub(super) runner: Runner,
    /// The runner's home: where its SSH key is (`ssh/id_ed25519`), the launcher's own.
    home: PathBuf,
    /// The lab's folder.
    pub(super) dir: PathBuf,
    /// What its module is.
    module: Module,
    /// The run's Terraform module.
    pub(super) project: PathBuf,
    pub(super) name: RunName,
    /// Whether its operations go to the home's runner service (a test's scripted runner runs them
    /// in this process).
    service: bool,
}

impl OpenedRun {
    /// The run of lab `id` in `dir`, whose Terraform module `project` is `module`, on a runner
    /// set up with `config`, its operations through the home's runner service when `service` (in
    /// this process for a test's).
    pub(super) fn new(config: RunnerConfig, dir: &Path, id: &str, (project, module): (PathBuf, Module), service: bool) -> Result<Self> {
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(errors::from_runner)?;
        let home = config.home.clone();
        Ok(OpenedRun { runner: Runner::new(config).map_err(errors::from_runner)?, home, dir: dir.to_path_buf(), module, project, name, service })
    }

    /// `act` on the run with `opts`, its events as `log`'s lines: through the home's runner
    /// service, else in this process (none reachable, or a test's runner).
    pub(super) async fn act(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        self.run(act, opts, log).await.map(drop).map_err(remote::Failure::into_error)
    }

    /// [`Self::act`], saying whether it reached its goal (false: a command it ran failed).
    pub(super) async fn run(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> std::result::Result<bool, remote::Failure> {
        if self.service
            && let Some(client) = remote::client(&self.home).await
        {
            let on = remote::Served { home: &self.home, client: &client, target: remote::target(&self.project, Format::Terraform, &self.name) };
            return Box::pin(remote::operate(on, act, opts, log)).await;
        }
        let lab = self.running().map_err(remote::Failure::Launcher)?;
        Box::pin(remote::in_process(&lab, act, opts, log)).await
    }

    /// The run's status, read by the home's runner service (else by the runner in this process).
    pub(super) async fn read(&self) -> Result<isothrum_runner::Status> {
        self.read_with(&RunOptions::new()).await
    }

    /// [`Self::read`], its tools given `opts`' values and secrets (a Proxmox server's API token,
    /// for its VMs' states on the server).
    pub(super) async fn read_with(&self, opts: &RunOptions) -> Result<isothrum_runner::Status> {
        if self.service
            && let Some(client) = remote::client(&self.home).await
        {
            let target = remote::target(&self.project, Format::Terraform, &self.name);
            return remote::status(&self.home, &client, &self.name, target, opts).await.map_err(remote::Failure::into_error);
        }
        self.running()?.status_with(opts).await.map_err(errors::from_runner)
    }

    /// Whether the module was generated (a start that failed before generation made none).
    pub(crate) fn has_project(&self) -> bool {
        std::fs::read_dir(&self.project).is_ok_and(|mut entries| entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "tf"))))
    }

    /// The Isoloom target the run's files are generated for; none for a module of the lab's own
    /// and for a run an earlier version left in a helper VM.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        match self.module {
            Module::Generated(target) => Some(target),
            Module::Own | Module::HelperVm => None,
        }
    }

    /// What the run's module is.
    pub(crate) fn module(&self) -> Module {
        self.module
    }

    /// The lab's module, opened under the run's name.
    pub(super) fn running(&self) -> Result<Running<'_>> {
        self.runner.open_as(&self.project, Format::Terraform, Some(self.name.clone())).map_err(|e| match self.module {
            Module::Generated(_) | Module::Own => errors::from_runner(e),
            // A helper VM an earlier version left on a server: said what to remove if not opened.
            Module::HelperVm => errors::of_helper_vm(e, self.name.as_str()),
        })
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<isothrum_runner::Status>> {
        if !self.has_project() {
            return Ok(None);
        }
        self.read().await.map(Some)
    }

    /// Brings the running lab up to date with its regenerated files, with `opts` (what changing
    /// the run needs: the place's connection and credentials).
    pub(crate) async fn update_with(&self, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        self.act(remote::Act::Update, opts, log).await
    }

    /// Takes `machine` out of the running lab ahead of its removal from the lab's files, with
    /// `opts`, as [`OpenedRun::update_with`].
    pub(crate) async fn retire_with(&self, machine: &str, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        self.act(remote::Act::Retire(machine), opts, log).await
    }

    /// The module's sign-in key for the machines it makes (Isoloom's Terraform modules ask for
    /// it, with no default): the runner's public key, made when there is none, as
    /// `ssh_public_key`, and its private key's file as `ssh_private_key_file`.
    pub(crate) async fn key_values(&self) -> Result<Vec<(String, String)>> {
        let public = self.runner.ssh_public_key().await.map_err(errors::from_runner)?;
        Ok(vec![("ssh_public_key".to_owned(), public.trim().to_owned()), ("ssh_private_key_file".to_owned(), self.key().to_string_lossy().into_owned())])
    }

    /// The runner's private key (the launcher's own): what the machines and a jump node let in.
    pub(crate) fn key(&self) -> PathBuf {
        self.home.join("ssh").join("id_ed25519")
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given: the runner's, an SSH along the route the module's outputs give (through its
    /// router, and through the jump host the run recorded first, `ssh_jump`), as the module's
    /// user, then a login shell as `user`.
    pub(crate) async fn console_command(&self, machine: &str, user: Option<&str>) -> Result<isothrum_runner::ShellCommand> {
        // Made when there is none: the key the module's machines let in.
        self.runner.ssh_public_key().await.map_err(errors::from_runner)?;
        self.running()?.console_command(machine, user).await.map_err(errors::from_runner)
    }

    /// The service endpoints the module publishes, by name, where this computer reaches each
    /// one (the runner's: through the run's jump host, a local forward it keeps), with `opts`.
    pub(crate) async fn endpoints(&self, opts: &RunOptions) -> Result<Vec<isothrum_runner::ServiceEndpoint>> {
        self.running()?.service_endpoints(opts).await.map_err(errors::from_runner)
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with `opts` (the
    /// place's credentials: a Proxmox server's API, as the module's own values carry them).
    pub(crate) async fn graphical_console(&self, machine: &str, opts: &RunOptions) -> Result<isothrum_runner::Console> {
        self.running()?.graphical_console(machine, opts).await.map_err(errors::from_runner)
    }

    /// What opens running VM `machine` in its hypervisor's own app (a server's web console), as
    /// the runner says with `opts`.
    pub(crate) async fn native_console(&self, machine: &str, opts: &RunOptions) -> Result<isothrum_runner::NativeConsole> {
        self.running()?.native_console(machine, opts).await.map_err(errors::from_runner)
    }
}
