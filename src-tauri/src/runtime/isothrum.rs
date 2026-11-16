//! The launcher on the Isothrum runner: every lab runs from the project Isoloom generates for its
//! target (the folder `.isoloom[-<n>]/docker/`, `.isoloom[-<n>]/vagrant/`, a Terraform module),
//! opened by the runner as a project of its format, through its `Running` operations: a container
//! lab on this machine's Docker, a VM lab on this machine's hypervisor (Vagrant, every local
//! provider) or an ESXi host, and a lab on a Proxmox server (`server`) or in a cloud account
//! (`cloud_runs`). The launcher alone knows the two products: what the lab says of itself (its
//! message, ports, roles, checks) it reads from the lab's spec through Isoloom, and it hands the
//! values a run takes to the runner as environment and secrets, never as files of the runner's.
//!
//! What stays the launcher's: the run's name (`cyberctf-<lab id>`, the Compose project the
//! launcher always used), the runner's home (`isothrum/` in the app data folder, the runner's
//! alone), the words the player reads (the runner's events in `progress`, its error codes in
//! `errors`) and the lab page's shapes (`view`). The attack box is no concern of the run's: the
//! launcher adds it to the lab as one of its machines (`attack_box`), and the run is brought
//! up to date with it ([`LabRun::update`], [`LabRun::retire`]) like with any change of the lab.
//! Everything goes to the home's runner service (`remote`), which this app hosts: an operation,
//! which it runs to its end whatever becomes of the app, a run's record, its consoles. Nothing
//! here opens the runner's store or runs a runner of its own.

mod checks;
mod claim;
mod cloud_runs;
mod described;
pub(crate) mod errors;
mod esxi;
mod gone;
mod old_engine;
mod opened;
#[cfg(feature = "cloud-services")]
mod own_module;
mod progress;
mod remote;
mod server;
mod tunnel;
mod view;
mod web;

pub(crate) use claim::CLAIM;
use claim::with_claim;
pub(crate) use cloud_runs::{CloudPlace, CloudRun, destroy_expired, expired_clouds};
pub(crate) use esxi::EsxiPlace;
pub(crate) use opened::Module;
pub(crate) use server::{ServerPlace, ServerRun};

/// A scripted Terraform `apply`, for the tests that run a module on scripted tools: it leaves its
/// state where it is asked to.
#[cfg(test)]
pub(crate) fn scripted_apply(call: &isothrum_runner::fake::Call) -> isothrum_runner::fake::Reply {
    if let Some(state) = call.args.iter().find_map(|a| a.strip_prefix("-state=")) {
        std::fs::write(state, "{}").unwrap();
    }
    isothrum_runner::fake::Reply::ok("")
}

/// The input variables the Terraform module in `dir` declares, read by the runner from the
/// module itself: nothing runs.
#[cfg(feature = "cloud-services")]
pub(crate) fn module_variables(dir: &Path) -> Result<Vec<isothrum_runner::InputVariable>> {
    isothrum_runner::variables_of(dir).map_err(|e| errors::coded(&e.report()))
}

/// The folder Isoloom generates a lab's files in, at its root (`.isoloom`, `.isoloom-<n>` for an
/// instance).
pub(crate) const OUTPUT_DIR: &str = ".isoloom";
/// The ESXi host's variables Isoloom's Vagrantfiles read.
pub(crate) const ESXI_HOSTNAME: &str = "ESXI_HOSTNAME";
/// The ESXi host's SSH port.
pub(crate) const ESXI_HOSTPORT: &str = "ESXI_HOSTPORT";
/// The variable Isoloom's Compose file reads to publish the lab's services on their own host ports
/// rather than free ones picked at random.
const PUBLISH_FIXED: &str = "ISOLOOM_PUBLISH_FIXED";

use std::path::{Path, PathBuf};

use isothrum_runner::{Format, RunName, RunOptions, SecretValue, Secrets, ShellCommand, Status};

use super::{LabStatus, Park, Place, Runtime, lab, paths};
use crate::error::{Error, Result};

/// Cancels what the home's runner service runs or has queued on lab `id`'s run, before a stop of
/// the lab.
pub(crate) async fn end_operations(dirs: &crate::platform::AppDirs, id: &str) {
    let (Ok(home), Ok(name)) = (paths::runner_home(dirs), RunName::new(&super::docker::project(id))) else { return };
    remote::cancel_under_way(&home, &name).await;
}

/// The labs whose run has an operation queued or running in the home's runner service: what an
/// update install waits for besides this process's own.
///
/// # Errors
///
/// The service's runs can't be listed.
pub(crate) async fn busy_in_service(dirs: &crate::platform::AppDirs) -> std::result::Result<Vec<String>, String> {
    let home = paths::runner_home(dirs).map_err(|e| e.to_string())?;
    let runs = remote::busy_runs(&home).await?;
    Ok(runs.iter().map(|r| super::docker::lab_of_project(r.as_str()).unwrap_or(r.as_str()).to_owned()).collect())
}

/// Whether lab `id`'s run ended its last operation failed, as the runner records it: its page
/// says the launch failed rather than drawing what is left of it.
pub(crate) async fn last_run_failed(dirs: &crate::platform::AppDirs, id: &str) -> bool {
    record_of(dirs, id).await.ok().flatten().is_some_and(|r| r.state == isothrum_runner::RunState::Failed)
}

/// Lab `id`'s run's operations as the home's runner service journals them, newest first, as JSON
/// (none when no service runs).
///
/// # Errors
///
/// The journal can't be read.
pub(crate) async fn journal_of(dirs: &crate::platform::AppDirs, id: &str) -> Result<serde_json::Value> {
    let home = paths::runner_home(dirs)?;
    let name = RunName::new(&super::docker::project(id)).map_err(|e| errors::coded(&e.report()))?;
    let rows = remote::journal(&home, &name).await.map_err(Error::Invalid)?;
    serde_json::to_value(rows).map_err(|e| Error::Invalid(e.to_string()))
}

/// Follows lab `id`'s run's operations under way in the home's runner service to their end, their
/// lines as `log`'s; nothing when none is under way.
///
/// # Errors
///
/// The service's stream can't be read.
pub(crate) async fn follow_under_way(dirs: &crate::platform::AppDirs, id: &str, log: &mut impl FnMut(String)) -> Result<()> {
    let home = paths::runner_home(dirs)?;
    let name = RunName::new(&super::docker::project(id)).map_err(|e| errors::coded(&e.report()))?;
    remote::follow_under_way(&home, &name, log).await.map_err(Error::Invalid)
}

/// Follows the home's runner service (started when none runs) for as long as its stream lasts,
/// telling `on` what another process asked of it and when a run leaves its standing state.
///
/// # Errors
///
/// No service can be reached, or its stream ended.
pub(crate) async fn follow_others(dirs: &crate::platform::AppDirs, on: &mut impl FnMut(remote::Others)) -> Result<()> {
    let home = paths::runner_home(dirs)?;
    let client = remote::client(&home).await?;
    remote::follow_others(&client, on).await.map_err(Error::Invalid)
}

pub(crate) use remote::{Others, records, rows};
#[cfg(test)]
pub(crate) use remote::{stop_service, unreachable as service_unreachable};

/// Lab `id`'s run's name (`cyberctf-<id>`).
fn run_name(id: &str) -> Result<RunName> {
    RunName::new(&super::docker::project(id)).map_err(|e| errors::coded(&e.report()))
}

/// Forgets lab `id`'s run in the runner's store, once the lab is removed: no run is left behind
/// for a lab that isn't there.
///
/// # Errors
///
/// No service can be reached, or it can't write its store.
pub(crate) async fn forget_run(home: &Path, id: &str) -> Result<()> {
    let Ok(name) = run_name(id) else { return Ok(()) };
    remote::forget_run(home, &name).await
}

/// Lab `id`'s run here, installed in `dir`, opened in the form its record says it was started in
/// (Docker, VMs, or the helper VM an earlier version ran a container lab in): none without a
/// record, or for a run recorded on a server or a cloud, which isn't this machine's to read.
pub(crate) async fn recorded_here(dirs: &crate::platform::AppDirs, dir: &Path, id: &str) -> Result<Option<LabRun>> {
    let Some(record) = record_of(dirs, id).await? else { return Ok(None) };
    let home = paths::runner_home(dirs)?;
    Ok(Some(match super::recorded::RunForm::of(&record) {
        super::recorded::RunForm::Docker => LabRun::docker(&home, dir, id)?,
        super::recorded::RunForm::HelperVm => LabRun::docker_in_vm(&home, dir, id)?,
        super::recorded::RunForm::Vagrant if record.host_label.is_none() => LabRun::vm(&home, dir, id)?,
        _ => return Ok(None),
    }))
}

/// Lab `id`'s run as the runner whose home is the app data folder records it (`cyberctf-<id>`),
/// if it has one: an unreadable record an error.
pub(crate) async fn record_of(dirs: &crate::platform::AppDirs, id: &str) -> Result<Option<isothrum_runner::RunRecord>> {
    // A lab whose id names no run (longer than a run's name may be) never ran.
    let Ok(name) = run_name(id) else { return Ok(None) };
    remote::record(&paths::runner_home(dirs)?, &name).await
}

/// What a run is: a container lab on Docker, a VM lab on a local hypervisor (a container lab's
/// VM form included), or a container lab an earlier version ran inside one helper VM (Isoloom's
/// former `docker-vm`), which Isoloom no longer generates: that run is read, stopped and removed
/// as it is, never generated again nor changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Docker,
    DockerVm,
    Vm,
}

/// One lab on this machine, run by the runner.
pub(crate) struct LabRun {
    /// The run, as the home's service knows it.
    served: remote::Served,
    dir: PathBuf,
    kind: Kind,
    /// The ESXi server a VM lab runs on, if not on this machine.
    esxi: Option<EsxiPlace>,
    /// The Docker context an earlier version noted for this container lab ([`old_engine`]),
    /// until an operation has the runner record it.
    old_engine: Option<String>,
}

impl LabRun {
    /// Container lab `id`, installed in `dir`, on the runner whose home is `home` (the app data
    /// folder), with this host's tools.
    pub(crate) fn docker(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::served(home, dir, id, Kind::Docker)
    }

    /// VM lab `id`, installed in `dir`, on the runner whose home is `home`, with this host's tools.
    pub(crate) fn vm(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::served(home, dir, id, Kind::Vm)
    }

    /// Container lab `id` left by an earlier version in one local helper VM (Isoloom's former
    /// `docker-vm`: Docker Compose inside a VM), installed in `dir`, on the runner whose home is
    /// `home`: to read, stop and remove that run.
    pub(crate) fn docker_in_vm(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::served(home, dir, id, Kind::DockerVm)
    }

    /// Lab `id`, installed in `dir` and declared `runtime`, as it runs on this machine, in the form
    /// its run's record says it was started in (`recorded`): on Docker, as VMs (a container lab
    /// started as one VM per machine included), or in the helper VM an earlier version left it in;
    /// none for a cloud-services lab, which runs only in a cloud account.
    pub(crate) fn local(home: &Path, dir: &Path, id: &str, runtime: Runtime, recorded: &super::recorded::Recorded) -> Result<Option<LabRun>> {
        Ok(Some(match recorded.form(runtime) {
            Runtime::Docker if recorded.helper_vm() => LabRun::docker_in_vm(home, dir, id)?,
            Runtime::Docker => LabRun::docker(home, dir, id)?,
            Runtime::Vm => LabRun::vm(home, dir, id)?,
            Runtime::Cloud => return Ok(None),
        }))
    }

    /// The Isoloom target the run's files are generated for; none for a helper VM an earlier
    /// version left ([`Kind::DockerVm`]), whose files Isoloom no longer generates.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        match self.kind {
            Kind::Docker => Some(lab::Target::Docker),
            Kind::DockerVm => None,
            Kind::Vm => Some(lab::Target::Vagrant),
        }
    }

    /// VM lab `id`, installed in `dir`, on the ESXi server `place`, through the runner whose home
    /// is `home`; a container lab's run there is the helper VM an earlier version left.
    pub(crate) fn on_esxi(home: &Path, dir: &Path, id: &str, runtime: Runtime, place: EsxiPlace) -> Result<LabRun> {
        let kind = if runtime == Runtime::Docker { Kind::DockerVm } else { Kind::Vm };
        Ok(LabRun::served(home, dir, id, kind)?.on(place))
    }

    /// The run on the ESXi server `place`.
    fn on(self, place: EsxiPlace) -> LabRun {
        LabRun { esxi: Some(place), ..self }
    }

    /// A run on a runner built from `config`, which this test process serves (tests script its
    /// tools).
    #[cfg(test)]
    fn with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, kind: Kind) -> Result<LabRun> {
        let home = config.home.clone();
        crate::testing::service::serve_home(config);
        LabRun::served(&home, dir, id, kind)
    }

    /// A run on the runner whose home is `home`, through the home's service.
    fn served(home: &Path, dir: &Path, id: &str, kind: Kind) -> Result<LabRun> {
        let (project, format) = match kind {
            Kind::Docker => (lab::docker_dir(dir), Format::Compose),
            Kind::DockerVm => (lab::docker_vm_dir(dir), Format::Vagrant),
            Kind::Vm => (lab::vagrant_dir(dir, Runtime::Vm), Format::Vagrant),
        };
        let served = remote::Served { home: home.to_path_buf(), name: run_name(id)?, project, format };
        let old_engine = if kind == Kind::Docker { old_engine::noted(dir) } else { None };
        Ok(LabRun { served, dir: dir.to_path_buf(), kind, esxi: None, old_engine })
    }

    /// `act` on the run with `opts`, its events as `log`'s lines, through the home's runner
    /// service. Whether it reached its goal (false: a check failed).
    async fn act(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> std::result::Result<bool, remote::Failure> {
        match Box::pin(self.served.operate(act, opts, log)).await {
            Err(e) => Err(self.refused(e).await),
            done => {
                // The runner recorded the engine an earlier version noted: the note goes.
                if self.old_engine.is_some() {
                    old_engine::forget(&self.dir);
                }
                done
            }
        }
    }

    /// `act` on the run with this place's options, as [`LabRun::act`], its failure in the
    /// player's words.
    async fn act_placed(&self, act: remote::Act<'_>, log: &mut impl FnMut(String)) -> Result<()> {
        self.act(act, &self.placed(RunOptions::new()), log).await.map(drop).map_err(|e| self.said(e.into_error()))
    }

    /// Whether the project's own file is there (`compose.yml`, the `Vagrantfile`).
    fn has_project(&self) -> bool {
        let file = if self.kind == Kind::Docker { "compose.yml" } else { "Vagrantfile" };
        self.served.project.join(file).is_file()
    }

    /// The service's failure `e` of the run: for the helper VM an earlier version left, the
    /// runner's refusal to open that form ([`LabRun::helper_vm_gone`]).
    async fn refused(&self, e: remote::Failure) -> remote::Failure {
        match e {
            remote::Failure::Service(report) if self.kind == Kind::DockerVm && errors::form_unsupported(&report) => {
                remote::Failure::Launcher(self.helper_vm_gone(&report).await)
            }
            other => other,
        }
    }

    /// What the lab says of itself (its machines, ports, networks, check runners), from its spec;
    /// nothing for the helper VM an earlier version left, whose form Isoloom no longer describes.
    fn described(&self) -> Result<described::Described> {
        match self.kind {
            Kind::Docker => described::Described::of(&self.dir, described::Form::Containers),
            Kind::Vm => described::Described::of(&self.dir, described::Form::Vms),
            Kind::DockerVm => Ok(described::Described::default()),
        }
    }

    /// The runner's refusal `report` to open the helper VM an earlier version left this lab in:
    /// it no longer runs that form, so nothing here reaches the VM. The run's record of it is
    /// forgotten (one on this machine), so its page reads it as stopped and its next start is in
    /// its true form, and the error says what to remove by hand.
    async fn helper_vm_gone(&self, report: &isothrum_runner::ErrorReport) -> Error {
        let said = errors::of_helper_vm(report, self.served.name.as_str());
        log::warn!("{}: {said}", self.dir.display());
        if self.esxi.is_none()
            && let Err(e) = self.served.forget().await
        {
            log::warn!("{}: its helper-VM record couldn't be forgotten: {e}", self.dir.display());
        }
        said
    }

    /// `opts` for this run's place: on an ESXi server, its id, its settings (as the environment
    /// Isoloom's Vagrantfiles read) and its password (a secret, beside the launch's); everywhere,
    /// the run's key under the name Isoloom's Vagrantfiles read it ([`super::lab::ISOLOOM_KEY`]),
    /// so the lab's VMs, networks and libvirt domains are named for this copy and no other; and
    /// the evidence claim's values every lab is given, empty when `opts` has none ([`with_claim`]);
    /// and the engine an earlier version noted for a container lab, unless `opts` names one.
    fn placed(&self, opts: RunOptions) -> RunOptions {
        let opts = with_claim(opts);
        let opts = opts.with_env(super::lab::ISOLOOM_KEY, isothrum_runner::copy_key(&self.served.home, &self.served.name));
        let opts = match (&self.old_engine, &opts.docker_context) {
            (Some(context), None) => opts.with_docker_context(context.clone()),
            _ => opts,
        };
        match &self.esxi {
            Some(place) => place.onto(opts),
            None => opts,
        }
    }

    /// Brings the lab up from its generated files (a parked run is resumed as it was), and says
    /// where to start: the lab's message, its host ports filled in. `inputs` are the launch's; a
    /// VM lab runs on `provider`.
    pub(crate) async fn up(&self, inputs: Inputs<'_>, provider: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let provider = provider.map(runner_provider).transpose()?;
        let opts = self.placed(start_options(inputs, provider));
        self.act(remote::Act::Up, &opts, log).await.map_err(|e| self.said(e.into_error()))?;
        described::log_message(&self.dir, self.target(), log);
        Ok(())
    }

    /// Brings the running lab up to date with its regenerated files (a machine added to the lab
    /// or taken out), with the parameters it was started with; a lab that isn't up is refused by
    /// the runner before anything acts. Its failure says whether the run may have changed.
    pub(crate) async fn update(&self, log: &mut impl FnMut(String)) -> std::result::Result<(), super::host::Failed> {
        self.act(remote::Act::Update, &self.placed(RunOptions::new()), log).await.map(drop).map_err(|f| f.failed(|e| self.said(e)))
    }

    /// Removes everything the run created: containers, networks, volumes, its pinned ports; or
    /// its VMs and Vagrant's record of them.
    pub(crate) async fn down(&self, log: &mut impl FnMut(String)) -> Result<()> {
        // A run whose files were never written (a start that failed before generation) created
        // nothing, so there is nothing to remove: a clean no-op, as the launcher's own Docker path
        // tolerates "no resource to remove". Without this, tearing down a failed launch errors on
        // the missing project.
        if !self.files_back().await? {
            log("Nothing to remove: the run has no files.".into());
            return Ok(());
        }
        self.act_placed(remote::Act::Down, log).await
    }

    /// Parks the lab, keeping what it created: Docker only stops containers, so both of the
    /// launcher's park modes are the runner's `Shutdown` there; VMs are suspended (`Pause`) or
    /// powered off (`Shutdown`).
    pub(crate) async fn park(&self, mode: Park, log: &mut impl FnMut(String)) -> Result<()> {
        let mode = match (self.kind, mode) {
            // VMs (a VM lab, or the VM a container lab runs in) suspend on Pause; Docker only
            // stops its containers, so both of its park modes are `Shutdown`.
            (Kind::Vm | Kind::DockerVm, Park::Pause) => isothrum_runner::Park::Pause,
            _ => isothrum_runner::Park::Shutdown,
        };
        self.act_placed(remote::Act::Park(mode), log).await
    }

    /// Starts the parked lab again. When nothing is left of it, the runner's record reads it
    /// absent, so the lab page offers a fresh start instead of a resume that can't work.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        match self.act(remote::Act::Resume, &self.placed(RunOptions::new()), log).await {
            Ok(_) => Ok(()),
            Err(remote::Failure::Service(report)) if errors::nothing_to_resume(&report) => Err(errors::coded(&report)),
            Err(e) => Err(self.said(e.into_error())),
        }
    }

    /// The lab as its page shows it; not running before its files were ever generated.
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        let place = if matches!(self.kind, Kind::Vm | Kind::DockerVm) { Place::LocalVm } else { Place::Container };
        if !self.files_back().await? {
            return Ok(LabStatus { place: Some(place), ..LabStatus::default() });
        }
        let status = self.read().await?;
        let parked = self.recorded().await?.as_ref().and_then(super::recorded::parked);
        Ok(LabStatus { place: Some(place), ..view::lab_status(&self.described()?, &status, parked) })
    }

    /// The run's status, read by the home's runner service, which reads a run one read at a
    /// time (the lab page's and the attack box's never ask Vagrant at once). The read's tools get
    /// the run's own values and secrets (an ESXi host's password).
    async fn read(&self) -> Result<Status> {
        match self.served.status(&self.placed(RunOptions::new())).await {
            Ok(status) => Ok(status),
            Err(e) => Err(self.said(self.refused(e).await.into_error())),
        }
    }

    /// The provider the run's record says it runs on, none before it ever ran: no tool runs.
    pub(crate) async fn recorded_provider(&self) -> Result<Option<String>> {
        Ok(self.recorded().await?.and_then(|r| r.provider).map(|p| isothrum_runner::wire_name(&p)))
    }

    /// The run's record, none before it ever ran: no tool runs.
    async fn recorded(&self) -> Result<Option<isothrum_runner::RunRecord>> {
        self.served.record().await
    }

    /// Runs the lab's set-up again on its running machines (`machine`, or all): its provisioners,
    /// and its controller's play. The launch's values aren't kept, so none reaches it: an input's
    /// file keeps the value it was written with.
    pub(crate) async fn provision(&self, machine: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        self.act_placed(remote::Act::Provision(machine, None), log).await
    }

    /// Takes `machine` out of the running lab, ahead of its removal from the lab's files.
    pub(crate) async fn retire(&self, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.act_placed(remote::Act::Retire(machine), log).await
    }

    /// Where the lab is reachable on this computer while it runs ([`web::local_url`]): none before
    /// its files exist, or while nothing of it is published.
    pub(crate) async fn local_url(&self) -> Result<Option<String>> {
        let Some(status) = self.run_status().await? else { return Ok(None) };
        Ok(web::local_url(&self.described()?, &status))
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<Status>> {
        if !self.files_back().await? {
            return Ok(None);
        }
        self.read().await.map(Some)
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given.
    pub(crate) async fn console_command(&self, machine: &str, user: Option<&str>) -> Result<ShellCommand> {
        match self.served.console_command(machine, user).await {
            Ok(command) => Ok(command),
            Err(e) => Err(self.refused(e).await.into_error()),
        }
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with the run's
    /// options (an ESXi host's settings and password), held with its tunnel until dropped.
    pub(crate) async fn graphical_console(&self, machine: &str) -> Result<isothrum_client::HeldConsole> {
        match self.served.graphical_console(machine, &self.placed(RunOptions::new())).await {
            Ok(console) => Ok(console),
            Err(e) => Err(self.refused(e).await.into_error()),
        }
    }

    /// What opens running VM `machine` in its hypervisor's own app, as the runner says.
    pub(crate) async fn native_console(&self, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        match self.served.native_console(machine, &self.placed(RunOptions::new())).await {
            Ok(console) => Ok(console),
            Err(e) => Err(self.refused(e).await.into_error()),
        }
    }
}

/// A graphical console's protocol (a run's offer, the runner's console) in the launcher's words:
/// `vnc`, `rdp`, or `none`.
pub(crate) fn protocol_word(protocol: isothrum_runner::ConsoleProtocol) -> &'static str {
    match protocol {
        isothrum_runner::ConsoleProtocol::Vnc => "vnc",
        isothrum_runner::ConsoleProtocol::Rdp => "rdp",
        _ => "none",
    }
}

/// What a start hands the lab: the launch's values for the lab's secret and input slots (a
/// value no slot takes is never delivered), and whether its services keep their own host ports
/// (`CTFd` on 8000) rather than free ones picked at random.
#[derive(Clone, Copy, Default)]
pub(crate) struct Inputs<'a> {
    /// The launch's values, by the name of the lab's slot each goes to.
    pub(crate) values: &'a [(String, String)],
    /// Whether its services keep their own host ports rather than free ones picked at random.
    pub(crate) default_ports: bool,
    /// The Docker context a container lab runs on (`orbstack`, `desktop-linux`…), when the player
    /// chose an engine other than the current one: the runner records it with the run, and every
    /// later operation and read of the run goes there, whatever the current context becomes.
    pub(crate) docker_context: Option<&'a str>,
}

/// The values' names only: a value can be a launch token or a flag.
impl std::fmt::Debug for Inputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inputs")
            .field("names", &self.values.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .field("default_ports", &self.default_ports)
            .field("docker_context", &self.docker_context)
            .finish()
    }
}

#[cfg(test)]
impl<'a> Inputs<'a> {
    /// The launch's `values`, on host ports picked at random.
    pub(crate) fn values(values: &'a [(String, String)]) -> Self {
        Inputs { values, ..Inputs::default() }
    }
}

/// A start's options: the launch's `inputs` (its values as secrets, and fixed host ports when the
/// player chose the lab's own), the hypervisor a VM lab runs on, and a parked run resumed rather
/// than rebuilt.
fn start_options(inputs: Inputs<'_>, provider: Option<isothrum_runner::Provider>) -> RunOptions {
    let mut secrets = Secrets::new();
    for (name, value) in inputs.values {
        secrets.insert(name.clone(), SecretValue::new(value.clone()));
    }
    let mut opts = RunOptions::new().with_secrets(secrets);
    if let Some(p) = provider {
        opts = opts.with_provider(p);
    }
    if let Some(context) = inputs.docker_context {
        opts = opts.with_docker_context(context);
    }
    if inputs.default_ports { opts.with_env(PUBLISH_FIXED, "1") } else { opts }
}

/// The runner's provider for the launcher's provider id `id` (`virtualbox`, `libvirt`…): the same
/// names on both sides; one the runner doesn't know is refused rather than dropped, so a start
/// never lands on a hypervisor the player didn't pick.
fn runner_provider(id: &str) -> Result<isothrum_runner::Provider> {
    isothrum_runner::from_wire_name(id).ok_or_else(|| Error::Invalid(format!("the runner knows no hypervisor `{id}`")))
}

#[cfg(test)]
mod test_lab;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vm_tests;
