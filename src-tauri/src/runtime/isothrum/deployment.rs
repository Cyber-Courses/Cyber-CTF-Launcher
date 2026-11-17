//! A lab's deployment: its run on the runner, opened under its own name from the project the
//! launcher generated for it, where it runs: on this machine (Docker, or VMs through Vagrant), on
//! an ESXi server (Vagrant there), on a Proxmox server or in a cloud account (a Terraform module).
//! One handle whatever the place: what differs is data on it, read where an operation needs it:
//! what its project is ([`Form`], with the runner's format of it), and where it runs ([`Site`]):
//! the options a change, a read and a console take, the words of a failure, and how the lab page shows the run.

use std::path::{Path, PathBuf};

use isothrum_runner::{Format, RunOptions};

use super::cloud_runs::CloudPlace;
use super::server::{self, ServerPlace};
use super::{EsxiPlace, Inputs, errors, remote, run_name, runner_provider, start_options};
use crate::error::{Error, Result};
use crate::runtime::docker::Check;
use crate::runtime::host::Failed;
use crate::runtime::{LabStatus, Runtime, lab};

/// What a run's project is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    /// The one Isoloom generates for a target.
    Generated(lab::Target),
    /// The lab's own, hand-written Terraform module (a cloud-services lab's `cloud/`).
    Own,
}

/// A run on this machine, as the constructors here name it: a container lab on Docker, or a VM
/// lab on a local hypervisor (a container lab's VM form included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Docker,
    Vm,
}

/// Where a deployment runs, with what reaching it takes.
#[derive(Debug, Clone)]
pub(crate) enum Site {
    /// On this machine.
    Here,
    /// On an ESXi server: its settings and password.
    Esxi(EsxiPlace),
    /// On a Proxmox server: its variables, and, for a change, its node and the SSH jump through
    /// it ([`crate::runtime::launch::server_place`]).
    Server(ServerPlace),
    /// In a cloud account: its region and credentials.
    Cloud(CloudPlace),
}

/// What a start hands the run: the launch's values, and the hypervisor a VM lab runs on.
#[derive(Clone, Copy)]
struct Start<'a> {
    inputs: Inputs<'a>,
    provider: Option<isothrum_runner::Provider>,
}

/// A lab's run on the runner, opened by its name from its project, where it runs.
pub(crate) struct Deployment {
    /// The run, as the home's service knows it (its project's folder and format).
    pub(super) served: remote::Served,
    /// The lab's folder.
    pub(super) dir: PathBuf,
    /// What its project is.
    form: Form,
    /// Where it runs.
    site: Site,
}

impl Deployment {
    /// The run of lab `id` in `dir`, whose project `project` (of `format`) is `form`, at `site`,
    /// on the runner whose home is `home`, through the home's runner service.
    fn new(home: &Path, dir: &Path, id: &str, (project, format, form): (PathBuf, Format, Form), site: Site) -> Result<Self> {
        let served = remote::Served { home: home.to_path_buf(), name: run_name(id)?, project, format };
        Ok(Deployment { served, dir: dir.to_path_buf(), form, site })
    }

    /// Lab `id`'s run of `kind`, installed in `dir`, at `site`.
    fn local(home: &Path, dir: &Path, id: &str, kind: Kind, site: Site) -> Result<Self> {
        let project = match kind {
            Kind::Docker => (lab::docker_dir(dir), Format::Compose, Form::Generated(lab::Target::Docker)),
            Kind::Vm => (lab::vagrant_dir(dir), Format::Vagrant, Form::Generated(lab::Target::Vagrant)),
        };
        Deployment::new(home, dir, id, project, site)
    }

    /// Lab `id`'s run of `kind` on this machine, installed in `dir`, on the runner whose home is
    /// `home` (the app data folder), with this host's tools.
    pub(super) fn here(home: &Path, dir: &Path, id: &str, kind: Kind) -> Result<Self> {
        Deployment::local(home, dir, id, kind, Site::Here)
    }

    /// Container lab `id`, installed in `dir`, on this machine's Docker.
    pub(crate) fn docker(home: &Path, dir: &Path, id: &str) -> Result<Self> {
        Deployment::here(home, dir, id, Kind::Docker)
    }

    /// VM lab `id`, installed in `dir`, on this machine's hypervisor.
    pub(crate) fn vm(home: &Path, dir: &Path, id: &str) -> Result<Self> {
        Deployment::here(home, dir, id, Kind::Vm)
    }

    /// Lab `id`, installed in `dir` and declared `runtime`, as it runs on this machine, in the form
    /// its run's record says it was started in (`recorded`): on Docker, or as VMs (a container lab
    /// started as one VM per machine included); none for a cloud-services lab, which runs only in a
    /// cloud account.
    pub(crate) fn local_of(home: &Path, dir: &Path, id: &str, runtime: Runtime, recorded: &crate::runtime::recorded::Recorded) -> Result<Option<Self>> {
        Ok(Some(match recorded.form(runtime) {
            Runtime::Docker => Deployment::docker(home, dir, id)?,
            Runtime::Vm => Deployment::vm(home, dir, id)?,
            Runtime::Cloud => return Ok(None),
        }))
    }

    /// VM lab `id`, installed in `dir`, on the ESXi server `place`: one VM per machine, through
    /// Vagrant there.
    pub(crate) fn on_esxi(home: &Path, dir: &Path, id: &str, place: EsxiPlace) -> Result<Self> {
        Deployment::local(home, dir, id, Kind::Vm, Site::Esxi(place))
    }

    /// Lab `id` of `runtime`, installed in `dir`, on the Proxmox server `place`.
    pub(crate) fn on_server(home: &Path, dir: &Path, id: &str, runtime: Runtime, place: ServerPlace) -> Result<Self> {
        // Proxmox's own module, one VM per machine.
        let (out, target) = lab::terraform(dir, runtime, "proxmox");
        Deployment::new(home, dir, id, (out, Format::Terraform, Form::Generated(target)), Site::Server(place))
    }

    /// Lab `id`, installed in `dir`, in the account `place`, its module the one generated for
    /// `runtime` on `cloud` (a container lab is `cloud-docker`, a VM lab `cloud-vm`).
    pub(crate) fn in_cloud(home: &Path, dir: &Path, id: &str, (runtime, cloud): (Runtime, &str), place: CloudPlace) -> Result<Self> {
        let (out, target) = lab::terraform(dir, runtime, cloud);
        Deployment::new(home, dir, id, (out, Format::Terraform, Form::Generated(target)), Site::Cloud(place))
    }

    /// Cloud-services lab `id`, installed in `dir`: its own module (`cloud/`), run as it is in the
    /// account `place`.
    #[cfg(feature = "cloud-services")]
    pub(crate) fn own(home: &Path, dir: &Path, id: &str, place: CloudPlace) -> Result<Self> {
        let module = crate::runtime::cloud_services::lab_file::module_dir(dir);
        Deployment::new(home, dir, id, (module, Format::Terraform, Form::Own), Site::Cloud(place))
    }

    /// The same run at `site`: a server's place as a change needs it (its node, its SSH jump),
    /// where reading the run takes its variables only.
    pub(crate) fn at(&self, site: Site) -> Self {
        Deployment { served: self.served.clone(), dir: self.dir.clone(), form: self.form, site }
    }

    /// Where the run is.
    pub(crate) fn site(&self) -> &Site {
        &self.site
    }

    /// What the run's project is.
    pub(crate) fn form(&self) -> Form {
        self.form
    }

    /// The run's project folder.
    pub(super) fn project(&self) -> &Path {
        &self.served.project
    }

    /// The Isoloom target the run's files are generated for; none for a module of the lab's own.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        match self.form {
            Form::Generated(target) => Some(target),
            Form::Own => None,
        }
    }

    /// Whether the project's own files are there: `compose.yml`, the `Vagrantfile`, a module's
    /// `.tf` files (a start that failed before generation made none).
    pub(crate) fn has_project(&self) -> bool {
        let project = self.project();
        match self.served.format {
            Format::Compose => project.join("compose.yml").is_file(),
            Format::Vagrant => project.join("Vagrantfile").is_file(),
            _ => std::fs::read_dir(project).is_ok_and(|mut entries| entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "tf")))),
        }
    }

    /// Whether the run's machines are VMs that suspend (a Vagrant project's).
    pub(super) fn suspends(&self) -> bool {
        self.served.format == Format::Vagrant
    }

    /// `act` on the run with `opts`, its events as `log`'s lines, through the home's runner
    /// service. Whether it reached its goal (false: a command it ran failed).
    pub(super) async fn run(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> std::result::Result<bool, remote::Failure> {
        Box::pin(self.served.operate(act, opts, log)).await
    }

    /// [`Deployment::run`], its failure in the player's words ([`Deployment::said`]).
    pub(super) async fn act(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        match self.run(act, opts, log).await {
            Ok(_) => Ok(()),
            Err(e) => Err(self.said(e)),
        }
    }

    /// The service's failure `e` of the run in the player's words: on a server, a refused sign-in
    /// names it.
    pub(super) fn said(&self, e: remote::Failure) -> Error {
        self.on_site(e.into_error())
    }

    /// `e` as the player reads it where the run is: on a server (ESXi, Proxmox), a refused sign-in
    /// names the server as Servers does.
    pub(super) fn on_site(&self, e: Error) -> Error {
        match &self.site {
            Site::Esxi(EsxiPlace { name, .. }) | Site::Server(ServerPlace { name, .. }) => errors::on_server(e, name),
            Site::Here | Site::Cloud(_) => e,
        }
    }

    /// `opts` for a run here or on ESXi: the run's key under the name Isoloom's Vagrantfiles read
    /// it ([`super::super::lab::ISOLOOM_KEY`]), so the lab's VMs, networks and libvirt domains are
    /// named for this copy and no other; on ESXi, its id, its settings (as the environment
    /// Isoloom's Vagrantfiles read) and its password (a secret, beside the launch's). The lab gets
    /// the values its launch gave and nothing else: an input given none is its spec's default
    /// (Isoloom's `inputs.*.default`), which is how a lab starts without a launch.
    pub(super) fn placed(&self, opts: RunOptions) -> RunOptions {
        let opts = opts.with_env(super::super::lab::ISOLOOM_KEY, isothrum_runner::copy_key(&self.served.home, &self.served.name));
        if let Site::Esxi(place) = &self.site { place.onto(opts) } else { opts }
    }

    /// What a change of the run runs with (its start's values `start` for a start): here and on ESXi, [`Deployment::placed`]; on a
    /// server, its place's ([`server::applying`]); in a cloud account, a start's are the account's
    /// values and credentials with the runner's key for the lab's instances, a later change's its
    /// credentials only (the runner replays what the start was given).
    async fn changing(&self, start: Option<Start<'_>>) -> Result<RunOptions> {
        let inputs = start.map_or_else(Inputs::default, |s| s.inputs);
        match &self.site {
            Site::Here | Site::Esxi(_) => Ok(self.placed(start.map_or_else(RunOptions::new, |s| start_options(s.inputs, s.provider)))),
            Site::Server(place) => Ok(server::applying(place, inputs, &self.key())),
            Site::Cloud(place) if start.is_some() => {
                // The key the lab's instances let in: a plain value the record keeps, so every
                // later operation replays it.
                let keys = self.key_values().await?;
                Ok(keys.into_iter().fold(place.options(inputs), |o, (k, v)| o.with_env(k, v)))
            }
            Site::Cloud(place) => Ok(place.later_options()),
        }
    }

    /// What a read of the run takes: here and on ESXi, [`Deployment::placed`] (an ESXi host's
    /// password); a server's credentials (its API, asked for its VMs' states); nothing in a cloud
    /// account.
    fn reading(&self) -> RunOptions {
        match &self.site {
            Site::Here | Site::Esxi(_) => self.placed(RunOptions::new()),
            Site::Server(place) => server::options(place, Inputs::default()),
            Site::Cloud(_) => RunOptions::new(),
        }
    }

    /// What a console of the run (a screen, the hypervisor's own app) is reached with: the place's
    /// credentials.
    fn reaching(&self) -> RunOptions {
        match &self.site {
            Site::Cloud(place) => place.later_options(),
            _ => self.reading(),
        }
    }

    /// The run's status, read by the home's runner service, which reads a run one read at a time.
    pub(super) async fn read(&self) -> Result<isothrum_runner::Status> {
        self.read_with(&self.reading()).await
    }

    /// [`Self::read`], its tools given `opts`' values and secrets.
    pub(super) async fn read_with(&self, opts: &RunOptions) -> Result<isothrum_runner::Status> {
        match self.served.status(opts).await {
            Ok(status) => Ok(status),
            Err(e) => Err(self.said(e)),
        }
    }

    /// The run's record, none before it ever ran: no tool runs.
    pub(super) async fn recorded(&self) -> Result<Option<isothrum_runner::RunRecord>> {
        self.served.record().await
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<isothrum_runner::Status>> {
        if !self.files_back().await? {
            return Ok(None);
        }
        self.read().await.map(Some)
    }

    /// Brings the lab up from its generated files (a parked run is resumed as it was), and says
    /// where to start: the lab's message, its host ports filled in. `inputs` are the launch's; a
    /// VM lab here or on ESXi runs on `provider`.
    pub(crate) async fn up(&self, inputs: Inputs<'_>, provider: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let provider = provider.map(runner_provider).transpose()?;
        let opts = self.changing(Some(Start { inputs, provider })).await?;
        self.act(remote::Act::Up, &opts, log).await?;
        // The ports the run got, which the message names: none read, the spec's.
        let run = self.read().await.ok();
        super::described::log_message(&self.dir, self.target(), run.as_ref(), log);
        Ok(())
    }

    /// `act` on the run as a change does ([`Deployment::changing`]), its failure in the player's
    /// words.
    async fn change(&self, act: remote::Act<'_>, log: &mut impl FnMut(String)) -> Result<()> {
        let opts = self.changing(None).await?;
        self.act(act, &opts, log).await
    }

    /// Removes everything the run created (containers, networks, volumes, its pinned ports; its
    /// VMs; what its module made), with the parameters it was applied with (the connection is
    /// the place's as it is now).
    pub(crate) async fn down(&self, log: &mut impl FnMut(String)) -> Result<()> {
        // A run whose files were never written (a start that failed before generation) created
        // nothing, so there is nothing to remove: a clean no-op. Without this, tearing down a
        // failed launch errors on the missing project.
        if !self.files_back().await? {
            log("Nothing to remove: the run has no files.".into());
            return Ok(());
        }
        self.change(remote::Act::Down, log).await
    }

    /// Brings the running lab up to date with its regenerated files (a machine added to the lab
    /// or taken out), with the parameters it was started with; a lab that isn't up is refused by
    /// the runner before anything acts. Its failure says whether the run may have changed.
    pub(crate) async fn update(&self, log: &mut impl FnMut(String)) -> std::result::Result<(), Failed> {
        let opts = self.changing(None).await.map_err(Failed::of_launcher)?;
        match self.run(remote::Act::Update, &opts, log).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let run_changed = e.run_changed();
                Err(Failed { error: self.said(e), run_changed })
            }
        }
    }

    /// Takes `machine` out of the running lab, ahead of its removal from the lab's files.
    pub(crate) async fn retire(&self, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.change(remote::Act::Retire(machine), log).await
    }

    /// Runs the lab's set-up again on its running machines (`machine`, or all): its provisioners,
    /// and its controller's play. The launch's values aren't kept, so none reaches it: an input's
    /// file keeps the value it was written with.
    pub(crate) async fn provision(&self, machine: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        self.change(remote::Act::Provision(machine, None), log).await
    }

    /// Parks the lab, keeping what it created, in the runner's mode of the same name: a Pause
    /// suspends VMs and freezes containers (`compose pause`), a Shutdown powers VMs off and stops
    /// containers.
    pub(crate) async fn park(&self, mode: crate::runtime::Park, log: &mut impl FnMut(String)) -> Result<()> {
        let mode = match mode {
            crate::runtime::Park::Pause => isothrum_runner::Park::Pause,
            crate::runtime::Park::Shutdown => isothrum_runner::Park::Shutdown,
        };
        self.change(remote::Act::Park(mode), log).await
    }

    /// Starts the parked lab again. When nothing is left of it, the runner's record reads it
    /// absent, so the lab page offers a fresh start instead of a resume that can't work.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        let opts = self.changing(None).await?;
        match self.run(remote::Act::Resume, &opts, log).await {
            Ok(_) => Ok(()),
            Err(remote::Failure::Service(report)) if errors::nothing_to_resume(&report) => Err(errors::coded(&report)),
            Err(e) => Err(self.said(e)),
        }
    }

    /// The lab as its page shows it; not running before its files were ever generated.
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        match &self.site {
            Site::Here | Site::Esxi(_) => self.local_status().await,
            Site::Server(place) => self.server_status(place).await,
            Site::Cloud(_) => self.cloud_status().await,
        }
    }

    /// What the lab says of itself (its machines, ports, networks, check runners), from its spec:
    /// its containers or its VMs as its project runs them; nothing for a module of the lab's own,
    /// which Isoloom didn't generate.
    pub(super) fn described(&self) -> Result<super::described::Described> {
        use super::described::{Described, Form as Of};
        match self.form {
            Form::Generated(lab::Target::Docker | lab::Target::CloudDocker) => Described::of(&self.dir, Of::Containers),
            Form::Generated(_) => Described::of(&self.dir, Of::Vms),
            Form::Own => Ok(Described::default()),
        }
    }

    /// The lab's checks: here and on ESXi, the check runners its spec declares
    /// ([`Deployment::lab_check`]); on a server, each runner its module's `checks` output names,
    /// run in its machine by the runner's `exec` over SSH ([`Deployment::module_check`]); in a
    /// cloud account, a cloud-services lab's pages, and none yet for a module Isoloom generates
    /// (the runner runs no command in a Terraform module's VMs there).
    pub(crate) async fn check(&self) -> Result<Check> {
        match (&self.site, self.form) {
            (Site::Here | Site::Esxi(_), _) => self.lab_check().await,
            (Site::Server(_), _) => self.module_check(&RunOptions::new()).await,
            #[cfg(feature = "cloud-services")]
            (Site::Cloud(_), Form::Own) => self.own_check().await,
            (Site::Cloud(_), _) => Ok(Check::default()),
        }
    }

    /// The module's sign-in key for the machines it makes ([`key_values`]).
    pub(crate) async fn key_values(&self) -> Result<Vec<(String, String)>> {
        key_values(&self.served.home).await
    }

    /// The runner's private key (the launcher's own): what the machines and a jump node let in.
    pub(crate) fn key(&self) -> PathBuf {
        key(&self.served.home)
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given: the runner's (for a module, an SSH along the route its outputs give: through
    /// its router, and through the jump host the run recorded first, `ssh_jump`, as the module's
    /// user, then a login shell as `user`).
    pub(crate) async fn console_command(&self, machine: &str, user: Option<&str>) -> Result<isothrum_runner::ShellCommand> {
        if self.served.format == Format::Terraform {
            // Made when there is none: the key the module's machines let in.
            remote::public_key(&self.served.home).await?;
        }
        match self.served.console_command(machine, user).await {
            Ok(command) => Ok(command),
            Err(e) => Err(self.said(e)),
        }
    }

    /// The service endpoints the module publishes, by name, where this computer reaches each
    /// one (the runner's: through the run's jump host, a local forward it keeps), with `opts`.
    pub(crate) async fn endpoints(&self, opts: &RunOptions) -> Result<Vec<isothrum_runner::ServiceEndpoint>> {
        match self.served.service_endpoints(opts).await {
            Ok(endpoints) => Ok(endpoints),
            Err(e) => Err(self.said(e)),
        }
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with the place's
    /// credentials, held with its tunnel until dropped.
    pub(crate) async fn graphical_console(&self, machine: &str) -> Result<isothrum_client::HeldConsole> {
        match self.served.graphical_console(machine, &self.reaching()).await {
            Ok(console) => Ok(console),
            Err(e) => Err(self.said(e)),
        }
    }

    /// What opens running VM `machine` in its hypervisor's own app (a server's web console), as
    /// the runner says with the place's credentials.
    pub(crate) async fn native_console(&self, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        match self.served.native_console(machine, &self.reaching()).await {
            Ok(console) => Ok(console),
            Err(e) => Err(self.said(e)),
        }
    }
}

/// Runs on runners this test process serves (tests script their tools).
#[cfg(test)]
impl Deployment {
    /// A run of `kind` here, on a runner built from `config`.
    pub(super) fn here_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, kind: Kind) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::here(&home, dir, id, kind)
    }

    /// [`Deployment::on_esxi`] of a VM lab, on a runner built from `config`.
    pub(super) fn on_esxi_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, place: EsxiPlace) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::on_esxi(&home, dir, id, place)
    }

    /// [`Deployment::on_server`], on a runner built from `config`.
    pub(crate) fn on_server_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, runtime: Runtime, place: ServerPlace) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::on_server(&home, dir, id, runtime, place)
    }

    /// [`Deployment::in_cloud`], on a runner built from `config`.
    pub(crate) fn in_cloud_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, module: (Runtime, &str), place: CloudPlace) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::in_cloud(&home, dir, id, module, place)
    }

    /// [`Deployment::own`], on a runner built from `config`.
    #[cfg(feature = "cloud-services")]
    pub(crate) fn own_with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, place: CloudPlace) -> Result<Self> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        Deployment::own(&home, dir, id, place)
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
