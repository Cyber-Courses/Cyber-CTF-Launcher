//! A lab's deployment: its run on the runner, opened under its own name from the project the
//! launcher generated for it, where it runs: a Terraform module on a Proxmox server or in a cloud
//! account. One handle whatever the place: what differs between places is data on it ([`Site`]),
//! read where an operation needs it: the options a change and a read take, the tunnel a change on
//! a server goes through, the words of a failure, and how the lab page shows the run.

use std::path::{Path, PathBuf};

use isothrum_runner::{Format, RunName, RunOptions};

use super::cloud_runs::CloudPlace;
use super::server::{self, ServerPlace};
use super::tunnel::Tunnel;
use super::{errors, remote};
use crate::error::{Error, Result};
use crate::runtime::docker::Check;
use crate::runtime::host::Failed;
use crate::runtime::{LabStatus, Runtime, lab};

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

/// Where a deployment runs, with what reaching it takes.
#[derive(Debug, Clone)]
pub(crate) enum Site {
    /// On a Proxmox server: its variables, and, for a change, its node and the SSH jump through it
    /// ([`super::super::launch::server_place`]).
    Server(ServerPlace),
    /// In a cloud account: its region and credentials.
    Cloud(CloudPlace),
}

/// A lab's run on the runner, opened by its name from its Terraform module, where it runs.
pub(crate) struct Deployment {
    /// The run, as the home's service knows it.
    pub(super) served: remote::Served,
    /// The lab's folder.
    pub(super) dir: PathBuf,
    /// What its module is.
    module: Module,
    /// The run's Terraform module.
    pub(super) project: PathBuf,
    /// Where it runs.
    site: Site,
}

impl Deployment {
    /// The run of lab `id` in `dir`, whose Terraform module `project` is `module`, at `site`, on
    /// the runner whose home is `home`, through the home's runner service.
    fn new(home: &Path, dir: &Path, id: &str, (project, module): (PathBuf, Module), site: Site) -> Result<Self> {
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(|e| errors::coded(&e.report()))?;
        let served = remote::Served { home: home.to_path_buf(), name, project: project.clone(), format: Format::Terraform };
        Ok(Deployment { served, dir: dir.to_path_buf(), module, project, site })
    }

    /// Lab `id` of `runtime`, installed in `dir`, on the Proxmox server `place`, on the runner
    /// whose home is `home` (the app data folder).
    pub(crate) fn on_server(home: &Path, dir: &Path, id: &str, runtime: Runtime, place: ServerPlace) -> Result<Self> {
        // A VM lab runs Proxmox's own module, one VM per machine. A container lab never starts
        // there: its run is the helper VM an earlier version left, read from where it ran.
        let module = if runtime == Runtime::Docker {
            (lab::docker_vm_dir(dir).join("proxmox"), Module::HelperVm)
        } else {
            let (out, target) = lab::terraform(dir, runtime, "proxmox");
            (out, Module::Generated(target))
        };
        Deployment::new(home, dir, id, module, Site::Server(place))
    }

    /// Lab `id`, installed in `dir`, in the account `place`, its module the one generated for
    /// `runtime` on `cloud` (a container lab is `cloud-docker`, a VM lab `cloud-vm`), on the
    /// runner whose home is `home`.
    pub(crate) fn in_cloud(home: &Path, dir: &Path, id: &str, (runtime, cloud): (Runtime, &str), place: CloudPlace) -> Result<Self> {
        let (out, target) = lab::terraform(dir, runtime, cloud);
        Deployment::new(home, dir, id, (out, Module::Generated(target)), Site::Cloud(place))
    }

    /// Cloud-services lab `id`, installed in `dir`: its own module (`cloud/`), run as it is in the
    /// account `place`, on the runner whose home is `home`.
    #[cfg(feature = "cloud-services")]
    pub(crate) fn own(home: &Path, dir: &Path, id: &str, place: CloudPlace) -> Result<Self> {
        let module = crate::runtime::cloud_services::lab_file::module_dir(dir);
        Deployment::new(home, dir, id, (module, Module::Own), Site::Cloud(place))
    }

    /// [`Deployment::on_server`] on a runner set up with `config`, which this test process
    /// serves (tests script its tools).
    #[cfg(test)]
    pub(crate) fn on_server_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, runtime: Runtime, place: ServerPlace) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::on_server(&home, dir, id, runtime, place)
    }

    /// [`Deployment::in_cloud`] on a runner set up with `config`, which this test process serves.
    #[cfg(test)]
    pub(crate) fn in_cloud_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, module: (Runtime, &str), place: CloudPlace) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::in_cloud(&home, dir, id, module, place)
    }

    /// [`Deployment::own`] on a runner set up with `config`, which this test process serves.
    #[cfg(all(test, feature = "cloud-services"))]
    pub(crate) fn own_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, place: CloudPlace) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::own(&home, dir, id, place)
    }

    /// The same run at `site`: a server's place as a change needs it (its node, its SSH jump),
    /// where reading the run takes its variables only.
    pub(crate) fn at(&self, site: Site) -> Self {
        Deployment { served: self.served.clone(), dir: self.dir.clone(), module: self.module, project: self.project.clone(), site }
    }

    /// Where the run is.
    #[cfg(any(test, feature = "cloud-services"))]
    pub(crate) fn site(&self) -> &Site {
        &self.site
    }

    /// `act` on the run with `opts`, its events as `log`'s lines, through the home's runner
    /// service.
    pub(super) async fn act(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        self.run(act, opts, log).await.map(drop).map_err(|e| self.said(e))
    }

    /// [`Self::act`], saying whether it reached its goal (false: a command it ran failed).
    pub(super) async fn run(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> std::result::Result<bool, remote::Failure> {
        Box::pin(self.served.operate(act, opts, log)).await
    }

    /// The service's failure `e` of the run in the player's words: for a helper VM an earlier
    /// version left on a server, what to remove when the runner no longer opens it; on a server,
    /// a refused sign-in names it ([`Deployment::on_site`]).
    pub(super) fn said(&self, e: remote::Failure) -> Error {
        let e = match (e, self.module) {
            (remote::Failure::Service(report), Module::HelperVm) => errors::of_helper_vm(&report, self.served.name.as_str()),
            (e, _) => e.into_error(),
        };
        self.on_site(e)
    }

    /// `e` as the player reads it where the run is: on a server, a refused sign-in names the
    /// server as Servers does.
    fn on_site(&self, e: Error) -> Error {
        match &self.site {
            Site::Server(place) => errors::on_server(e, &place.name),
            Site::Cloud(_) => e,
        }
    }

    /// What a change of the run (its start, a stop, an update, a machine retired) runs with, the
    /// launch's values `env` among them, and the tunnel it goes through, held until it ends: on a
    /// server, its place's ([`server::applying`]); in a cloud account, a start's are the account's
    /// values and credentials with the runner's key for the lab's instances, a later change's its
    /// credentials only (the runner replays what the start was given).
    async fn changing(&self, act: &remote::Act<'_>, env: &[(String, String)]) -> Result<(RunOptions, Option<Tunnel>)> {
        match &self.site {
            Site::Server(place) => server::applying(place, env, &self.key()).await,
            Site::Cloud(place) if matches!(act, remote::Act::Up) => {
                // The key the lab's instances let in: a plain value the record keeps, so every
                // later operation replays it.
                let keys = self.key_values().await?;
                Ok((keys.into_iter().fold(place.options(env), |o, (k, v)| o.with_env(k, v)), None))
            }
            Site::Cloud(place) => Ok((place.later_options(), None)),
        }
    }

    /// What a console of the run (a screen, the hypervisor's own app) is reached with: the
    /// place's credentials.
    fn reaching(&self) -> RunOptions {
        match &self.site {
            Site::Server(place) => server::options(place, &[]),
            Site::Cloud(place) => place.later_options(),
        }
    }

    /// The run's status, read by the home's runner service.
    pub(super) async fn read(&self) -> Result<isothrum_runner::Status> {
        self.read_with(&RunOptions::new()).await
    }

    /// [`Self::read`], its tools given `opts`' values and secrets (a Proxmox server's API token,
    /// for its VMs' states on the server).
    pub(super) async fn read_with(&self, opts: &RunOptions) -> Result<isothrum_runner::Status> {
        self.served.status(opts).await.map_err(|e| self.said(e))
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

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<isothrum_runner::Status>> {
        if !self.has_project() {
            return Ok(None);
        }
        self.read().await.map(Some)
    }

    /// Brings the lab up from its generated module, with the launch's values `env` (the inputs the
    /// lab's slots take), and says where to start.
    pub(crate) async fn up(&self, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        let act = remote::Act::Up;
        let (opts, _tunnel) = self.changing(&act, env).await?;
        self.act(act, &opts, log).await?;
        super::described::log_message(&self.dir, self.target(), log);
        Ok(())
    }

    /// Removes everything the run created, with the parameters it was applied with (the
    /// connection is the place's as it is now).
    pub(crate) async fn down(&self, log: &mut impl FnMut(String)) -> Result<()> {
        // A run whose module was never generated created nothing: a clean no-op, as the other
        // runner paths tolerate (without this, tearing down a lab that was never deployed errors
        // on the missing module).
        if !self.has_project() {
            log("Nothing to remove: the run has no files.".to_owned());
            return Ok(());
        }
        let act = remote::Act::Down;
        let (opts, _tunnel) = self.changing(&act, &[]).await?;
        self.act(act, &opts, log).await
    }

    /// Brings the running lab up to date with its regenerated module (a machine added to the lab
    /// or taken out); the machines that run keep what they were given.
    pub(crate) async fn update(&self, log: &mut impl FnMut(String)) -> std::result::Result<(), Failed> {
        let act = remote::Act::Update;
        let (opts, _tunnel) = self.changing(&act, &[]).await.map_err(Failed::of_launcher)?;
        self.run(act, &opts, log).await.map(drop).map_err(|f| f.failed(|e| self.on_site(e)))
    }

    /// Takes `machine` out of the running lab ahead of its removal from the module.
    pub(crate) async fn retire(&self, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        let act = remote::Act::Retire(machine);
        let (opts, _tunnel) = self.changing(&act, &[]).await?;
        self.act(act, &opts, log).await
    }

    /// The lab as its page shows it; not running before its module was ever generated.
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        match &self.site {
            Site::Server(place) => self.server_status(place).await,
            Site::Cloud(_) => self.cloud_status().await,
        }
    }

    /// The lab's checks: on a server, each runner its module's `checks` output names, run in its
    /// machine by the runner's `exec` over SSH, through the server's node first when only it
    /// reaches the lab (the run's recorded `ssh_jump`) ([`Deployment::module_check`]); in a cloud
    /// account, a cloud-services lab's pages, and none yet for a module Isoloom generates (the
    /// runner runs no command in a Terraform module's VMs there).
    pub(crate) async fn check(&self) -> Result<Check> {
        match (&self.site, self.module) {
            (Site::Server(_), _) => self.module_check(&RunOptions::new()).await,
            #[cfg(feature = "cloud-services")]
            (Site::Cloud(_), Module::Own) => self.own_check().await,
            (Site::Cloud(_), _) => Ok(Check::default()),
        }
    }

    /// The module's sign-in key for the machines it makes (Isoloom's Terraform modules ask for
    /// it, with no default): the runner's ([`key_values`]).
    pub(crate) async fn key_values(&self) -> Result<Vec<(String, String)>> {
        key_values(&self.served.home).await
    }

    /// The runner's private key (the launcher's own): what the machines and a jump node let in.
    pub(crate) fn key(&self) -> PathBuf {
        key(&self.served.home)
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given: the runner's, an SSH along the route the module's outputs give (through its
    /// router, and through the jump host the run recorded first, `ssh_jump`), as the module's
    /// user, then a login shell as `user`.
    pub(crate) async fn console_command(&self, machine: &str, user: Option<&str>) -> Result<isothrum_runner::ShellCommand> {
        // Made when there is none: the key the module's machines let in.
        remote::public_key(&self.served.home).await?;
        self.served.console_command(machine, user).await.map_err(|e| self.said(e))
    }

    /// The service endpoints the module publishes, by name, where this computer reaches each
    /// one (the runner's: through the run's jump host, a local forward it keeps), with `opts`.
    pub(crate) async fn endpoints(&self, opts: &RunOptions) -> Result<Vec<isothrum_runner::ServiceEndpoint>> {
        self.served.service_endpoints(opts).await.map_err(|e| self.said(e))
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with the place's
    /// credentials (a Proxmox server's API, as the module's own values carry them).
    pub(crate) async fn graphical_console(&self, machine: &str) -> Result<isothrum_client::HeldConsole> {
        self.served.graphical_console(machine, &self.reaching()).await.map_err(|e| self.said(e))
    }

    /// What opens running VM `machine` in its hypervisor's own app (a server's web console), as
    /// the runner says with the place's credentials.
    pub(crate) async fn native_console(&self, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        self.served.native_console(machine, &self.reaching()).await.map_err(|e| self.said(e))
    }
}

/// The sign-in key of the machines a Terraform module makes (Isoloom's modules ask for it, with
/// no default): the runner's public key of home `home`, made when there is none, as
/// `ssh_public_key`, and its private key's file as `ssh_private_key_file`.
///
/// # Errors
///
/// No service can be reached, or it can't make the key.
pub(crate) async fn key_values(home: &Path) -> Result<Vec<(String, String)>> {
    let public = remote::public_key(home).await?;
    Ok(vec![("ssh_public_key".to_owned(), public.trim().to_owned()), ("ssh_private_key_file".to_owned(), key(home).to_string_lossy().into_owned())])
}

/// The runner's private key of home `home` (the launcher's own).
fn key(home: &Path) -> PathBuf {
    home.join("ssh").join("id_ed25519")
}
