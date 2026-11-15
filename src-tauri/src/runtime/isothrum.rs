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
//! An operation goes to the home's runner service (`remote`), which this app hosts and which runs
//! it to its end whatever becomes of the app.

mod checks;
mod cloud_runs;
mod described;
mod engine;
pub(crate) mod errors;
mod esxi;
mod gate;
mod gone;
mod one_read;
mod opened;
#[cfg(feature = "cloud-services")]
mod own_module;
mod progress;
mod remote;
mod retry;
mod runner_said;
mod server;
mod view;

pub(crate) use cloud_runs::{CloudPlace, CloudRun, reap_expired as reap_expired_clouds};
use engine::PROCESSES;
pub(crate) use engine::{ENGINE_VAR, EngineProcesses};
pub(crate) use esxi::EsxiPlace;
pub(crate) use opened::Module;
pub(crate) use server::{ServerPlace, ServerRun};

/// The input variables the Terraform module in `dir` declares, read by the runner from the
/// module itself: nothing runs.
#[cfg(feature = "cloud-services")]
pub(crate) fn module_variables(dir: &Path) -> Result<Vec<isothrum_runner::InputVariable>> {
    isothrum_runner::variables_of(dir).map_err(errors::from_runner)
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

/// The form a run was started in, as the runner records it: its project's format and folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunForm {
    /// A Compose project: containers on Docker.
    Docker,
    /// The helper VM an earlier version ran a container lab in (Isoloom's former `docker-vm`).
    HelperVm,
    /// A Vagrant project: VMs.
    Vagrant,
    /// A Terraform module: a server's or a cloud account's.
    Terraform,
}

impl RunForm {
    /// The form of `record`'s run: by its format, and by its folder for the helper VM.
    pub(crate) fn of(record: &isothrum_runner::RunRecord) -> RunForm {
        match record.format {
            Format::Compose => RunForm::Docker,
            Format::Vagrant if record.project.components().any(|c| c.as_os_str() == lab::DOCKER_VM_FOLDER) => RunForm::HelperVm,
            Format::Vagrant => RunForm::Vagrant,
            _ => RunForm::Terraform,
        }
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;

use isothrum_runner::{Format, RunName, RunOptions, Runner, RunnerConfig, Running, SecretValue, Secrets, ShellCommand, Status};

use super::{LabStatus, Park, Place, Runtime, lab, paths};
use crate::error::{Error, Result};

/// How long an operation the player asked for waits for the run's lock: the page's own status
/// checks (a few seconds each, every so often) hold it too, and a Pause clicked while one runs
/// must wait for it, not fail with "another operation is in progress".
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
/// Between two tries for the lock.
const LOCK_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether `e` is the run's lock held by another operation, with time left to wait for it.
fn lock_held(e: &isothrum_runner::Error, deadline: std::time::Instant) -> bool {
    e.code() == isothrum_runner::ErrorCode::Locked && std::time::Instant::now() < deadline
}

/// Runs `op` (a runner operation given the sink of [`progress::channel`]), its lines reaching
/// `log`; tried again while another operation holds the run's lock, for up to [`LOCK_WAIT`].
/// The sink is `op`'s to own (not lent) so the operation's future stays `Send`.
async fn when_unlocked<T, F>(log: &mut impl FnMut(String), mut op: impl FnMut(progress::Lines) -> F) -> std::result::Result<T, isothrum_runner::Error>
where
    F: Future<Output = std::result::Result<T, isothrum_runner::Error>>,
{
    let deadline = std::time::Instant::now() + LOCK_WAIT;
    loop {
        let (sink, lines) = progress::channel();
        match progress::follow(&mut *log, lines, op(sink)).await {
            Err(e) if lock_held(&e, deadline) => tokio::time::sleep(LOCK_POLL).await,
            r => return r,
        }
    }
}

/// The Vagrant plugin `provider` needs, from the runner's table (`isothrum_runner::Provider::plugin`):
/// none when Vagrant has it built in, and none for a provider Isoloom's Vagrantfiles don't drive
/// (Proxmox, the clouds), which the caller knows.
pub(crate) fn vagrant_plugin(provider: super::providers::Provider) -> Option<&'static str> {
    vagrant_provider(provider)?.plugin()
}

/// `provider` as the runner names it, when Isoloom's Vagrantfiles drive it.
fn vagrant_provider(provider: super::providers::Provider) -> Option<isothrum_runner::Provider> {
    use super::providers::Provider as P;
    use isothrum_runner::Provider as Isoloom;
    Some(match provider {
        P::Virtualbox => Isoloom::Virtualbox,
        P::VmwareDesktop => Isoloom::VmwareDesktop,
        P::Hyperv => Isoloom::Hyperv,
        P::Parallels => Isoloom::Parallels,
        P::Libvirt => Isoloom::Libvirt,
        P::Qemu => Isoloom::Qemu,
        P::Utm => Isoloom::Utm,
        P::VmwareEsxi => Isoloom::VmwareEsxi,
        P::Proxmox | P::Aws | P::Azure | P::Gcp | P::DigitalOcean | P::Linode | P::Oci => return None,
    })
}

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
pub(crate) fn last_run_failed(dirs: &crate::platform::AppDirs, id: &str) -> bool {
    record_of(dirs, id).ok().flatten().is_some_and(|r| r.state == isothrum_runner::RunState::Failed)
}

/// Lab `id`'s run's operations as the home's runner service journals them, newest first, as JSON
/// (none when no service runs).
///
/// # Errors
///
/// The journal can't be read.
pub(crate) async fn journal_of(dirs: &crate::platform::AppDirs, id: &str) -> Result<serde_json::Value> {
    let home = paths::runner_home(dirs)?;
    let name = RunName::new(&super::docker::project(id)).map_err(errors::from_runner)?;
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
    let name = RunName::new(&super::docker::project(id)).map_err(errors::from_runner)?;
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
    let client = remote::client(&home).await.ok_or_else(|| Error::Invalid("no runner service can be reached".into()))?;
    remote::follow_others(&client, on).await.map_err(Error::Invalid)
}

pub(crate) use remote::Others;

/// Every run the runner whose home is `home` records, each in the state its last operation left
/// it in, as its store says.
pub(crate) fn records(home: &Path) -> Result<Vec<isothrum_runner::RunRecord>> {
    Runner::new(host_config(home)).map_err(errors::from_runner)?.runs().map_err(errors::from_runner)
}

/// Forgets lab `id`'s run in the runner's store, once the lab is removed: no run is left behind
/// for a lab that isn't there.
///
/// # Errors
///
/// The runner's store can't be opened or written.
pub(crate) fn forget_run(home: &Path, id: &str) -> Result<()> {
    let name = RunName::new(&super::docker::project(id)).map_err(errors::from_runner)?;
    isothrum_runner::store::Store::open(home).and_then(|store| store.write(|tx| tx.forget_run(&name))).map_err(errors::from_runner)
}

/// Lab `id`'s run here, installed in `dir`, opened in the form its record says it was started in
/// (Docker, VMs, or the helper VM an earlier version ran a container lab in): none without a record, or for a run recorded on a server
/// or a cloud, which isn't this machine's to read.
pub(crate) fn recorded_here(dirs: &crate::platform::AppDirs, dir: &Path, id: &str) -> Result<Option<LabRun>> {
    let Some(record) = record_of(dirs, id)? else { return Ok(None) };
    let home = paths::runner_home(dirs)?;
    Ok(Some(match RunForm::of(&record) {
        RunForm::Docker => LabRun::docker(&home, dir, id)?,
        RunForm::HelperVm => LabRun::docker_in_vm(&home, dir, id)?,
        RunForm::Vagrant if record.host_label.is_none() => LabRun::vm(&home, dir, id)?,
        _ => return Ok(None),
    }))
}

/// Lab `id`'s run as the runner whose home is the app data folder records it (`cyberctf-<id>`),
/// if it has one: read from its files only, an unreadable record an error.
pub(crate) fn record_of(dirs: &crate::platform::AppDirs, id: &str) -> Result<Option<isothrum_runner::RunRecord>> {
    let name = super::docker::project(id);
    Ok(records(&paths::runner_home(dirs)?)?.into_iter().find(|r| r.name.as_str() == name))
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
    runner: Runner,
    dir: PathBuf,
    name: RunName,
    kind: Kind,
    /// The ESXi server a VM lab runs on, if not on this machine.
    esxi: Option<EsxiPlace>,
    /// The Docker context a container lab's launch chose (its engine marker), if any.
    engine: Option<String>,
    /// The runner's home (its service's).
    home: PathBuf,
    /// Whether its operations go to the home's runner service (a test's scripted runner runs them
    /// in this process).
    service: bool,
}

impl LabRun {
    /// Container lab `id`, installed in `dir`, on the runner whose home is `home` (the app data
    /// folder), with this host's tools.
    pub(crate) fn docker(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        // The lab's docker calls go to the engine chosen at launch (its marker), if any.
        Ok(LabRun { engine: paths::engine(dir), ..LabRun::served(home, dir, id, Kind::Docker)? })
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
    /// it was started in (`paths::local_runtime`): on Docker, as VMs (a container lab started as
    /// one VM per machine included), or in the helper VM an earlier version left it in; none for a
    /// cloud-services lab, which runs only in a cloud account.
    pub(crate) fn local(home: &Path, dir: &Path, id: &str, runtime: Runtime) -> Result<Option<LabRun>> {
        Ok(Some(match paths::local_runtime(dir, runtime) {
            Runtime::Docker if paths::local_vm(dir).is_some() => LabRun::docker_in_vm(home, dir, id)?,
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

    /// A run on the runner whose home is `home`, its operations through the home's service.
    fn served(home: &Path, dir: &Path, id: &str, kind: Kind) -> Result<LabRun> {
        Ok(LabRun { service: true, ..LabRun::with(host_config(home), dir, id, kind)? })
    }

    /// A run on a runner built from `config`, its operations in this process (tests script its
    /// tools).
    fn with(config: RunnerConfig, dir: &Path, id: &str, kind: Kind) -> Result<LabRun> {
        let name = RunName::new(&super::docker::project(id)).map_err(errors::from_runner)?;
        let home = config.home.clone();
        let runner = Runner::new(config).map_err(errors::from_runner)?;
        Ok(LabRun { runner, dir: dir.to_path_buf(), name, kind, esxi: None, engine: None, home, service: false })
    }

    /// `act` on the run with `opts`, its events as `log`'s lines: through the home's runner
    /// service, else in this process (none reachable, or a test's runner). Whether it reached
    /// its goal (false: a check failed).
    async fn act(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> std::result::Result<bool, remote::Failure> {
        if self.service
            && let Some(client) = remote::client(&self.home).await
        {
            let on = remote::Served { home: &self.home, client: &client, target: remote::target(&self.project(), self.format(), &self.name) };
            return Box::pin(remote::operate(on, act, opts, log)).await;
        }
        let lab = self.lab().map_err(remote::Failure::Launcher)?;
        Box::pin(remote::in_process(&lab, act, opts, log)).await
    }

    /// `act` on the run with this place's options, as [`LabRun::act`], its failure in the
    /// player's words.
    async fn act_placed(&self, act: remote::Act<'_>, log: &mut impl FnMut(String)) -> Result<()> {
        self.act(act, &self.placed(RunOptions::new()), log).await.map(drop).map_err(|e| self.said(e.into_error()))
    }

    /// The lab's generated project: its Compose folder, or its Vagrant one.
    fn project(&self) -> PathBuf {
        match self.kind {
            Kind::Docker => lab::docker_dir(&self.dir),
            Kind::DockerVm => lab::docker_vm_dir(&self.dir),
            Kind::Vm => lab::vagrant_dir(&self.dir, Runtime::Vm),
        }
    }

    /// The project's format.
    fn format(&self) -> Format {
        if self.kind == Kind::Docker { Format::Compose } else { Format::Vagrant }
    }

    /// Whether the project's own file is there (`compose.yml`, the `Vagrantfile`).
    fn has_project(&self) -> bool {
        let file = if self.kind == Kind::Docker { "compose.yml" } else { "Vagrantfile" };
        self.project().join(file).is_file()
    }

    /// The lab's project, opened under the run's name. A Compose or Vagrant project generated by
    /// an earlier version is still one of its format: it is opened as it is.
    fn lab(&self) -> Result<Running<'_>> {
        match self.runner.open_as(&self.project(), self.format(), Some(self.name.clone())) {
            Ok(lab) => Ok(lab),
            Err(e) if self.kind == Kind::DockerVm => Err(self.helper_vm_gone(e)),
            Err(e) => Err(errors::from_runner(e)),
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

    /// The runner's refusal `e` to open the helper VM an earlier version left this lab in. When it
    /// no longer runs that form, nothing here reaches the VM: the lab's helper-VM mark is cleared,
    /// so its page reads it as stopped and its next start is in its true form, and the error says
    /// what to remove by hand.
    fn helper_vm_gone(&self, e: isothrum_runner::Error) -> Error {
        let unsupported = e.code() == isothrum_runner::ErrorCode::Unsupported;
        let said = errors::of_helper_vm(e, self.name.as_str());
        if unsupported {
            log::warn!("{}: {said}", self.dir.display());
            if let Err(e) = paths::mark_local_vm(&self.dir, None) {
                log::warn!("{}: its helper-VM mark couldn't be cleared: {e}", self.dir.display());
            }
        }
        said
    }

    /// `opts` for this run's place: on an ESXi server, its id, its settings (as the environment
    /// Isoloom's Vagrantfiles read) and its password (a secret, beside the launch's); everywhere,
    /// the run's key under the name Isoloom's Vagrantfiles read it ([`super::lab::ISOLOOM_KEY`]),
    /// so the lab's VMs, networks and libvirt domains are named for this copy and no other.
    fn placed(&self, opts: RunOptions) -> RunOptions {
        let opts = opts.with_env(super::lab::ISOLOOM_KEY, self.runner.copy_key(&self.name));
        let opts = match &self.engine {
            Some(context) => opts.with_env(ENGINE_VAR, context.clone()),
            None => opts,
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
        self.refuse_up_when_running()?;
        let opts = self.placed(start_options(inputs, provider));
        self.act(remote::Act::Up, &opts, log).await.map_err(|e| self.said(e.into_error()))?;
        described::log_message(&self.dir, self.target(), log);
        Ok(())
    }

    /// Removes everything the run created: containers, networks, volumes, its pinned ports; or
    /// its VMs and Vagrant's record of them.
    pub(crate) async fn down(&self, log: &mut impl FnMut(String)) -> Result<()> {
        // A run whose manifest was never written (a start that failed before generation) created
        // nothing, so there is nothing to remove: a clean no-op, as the launcher's own Docker path
        // tolerates "no resource to remove". Without this, tearing down a failed launch errors on
        // the missing project.
        if !self.files_back()? {
            log("Nothing to remove: the run has no files.".into());
            return Ok(());
        }
        self.act_placed(remote::Act::Down, log).await
    }

    /// Parks the lab, keeping what it created: Docker only stops containers, so both of the
    /// launcher's park modes are the manifest's `shutdown` there; VMs are suspended (`Pause`) or
    /// powered off (`Shutdown`).
    pub(crate) async fn park(&self, mode: Park, log: &mut impl FnMut(String)) -> Result<()> {
        let mode = match (self.kind, mode) {
            // VMs (a VM lab, or the VM a container lab runs in) suspend on Pause; Docker only
            // stops its containers, so both of its park modes are the manifest's `shutdown`.
            (Kind::Vm | Kind::DockerVm, Park::Pause) => isothrum_runner::Park::Pause,
            _ => isothrum_runner::Park::Shutdown,
        };
        self.act_placed(remote::Act::Park(mode), log).await
    }

    /// Starts the parked lab again. When nothing is left of it, the launcher's parked mark goes
    /// too, so the lab page offers a fresh start instead of a resume that can't work.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        self.require_something_to_resume().await?;
        match self.act(remote::Act::Resume, &self.placed(RunOptions::new()), log).await {
            Ok(_) => Ok(()),
            Err(e) if e.code() == Some(isothrum_runner::ErrorCode::NothingToResume) => {
                paths::mark_parked(&self.dir, None)?;
                Err(e.into_error())
            }
            Err(e) => Err(self.said(e.into_error())),
        }
    }

    /// The lab as its page shows it; not running before its files were ever generated.
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        let place = if matches!(self.kind, Kind::Vm | Kind::DockerVm) { Place::LocalVm } else { Place::Container };
        if !self.files_back()? {
            return Ok(LabStatus { place: Some(place), ..LabStatus::default() });
        }
        let status = self.read().await?;
        Ok(LabStatus { place: Some(place), ..view::lab_status(&self.described()?, &status, paths::parked(&self.dir)) })
    }

    /// The run's status, read by the home's runner service (else by the runner in this process),
    /// one read of the run at a time in the app ([`one_read`]): the lab page's and the attack
    /// box's never ask Vagrant at once.
    async fn read(&self) -> Result<Status> {
        one_read::status(&self.dir.join(self.name.as_str()), || async {
            if self.service
                && let Some(client) = remote::client(&self.home).await
            {
                let target = remote::target(&self.project(), self.format(), &self.name);
                return remote::status(&self.home, &client, &self.name, target).await.map_err(|e| self.said(e.into_error()));
            }
            self.lab()?.status().await.map_err(|e| self.said(errors::from_runner(e)))
        })
        .await
    }

    /// The provider the run's record says it runs on, none before it ever ran: read from the
    /// runner's files only, no tool runs.
    pub(crate) fn recorded_provider(&self) -> Result<Option<String>> {
        let runs = self.runner.runs().map_err(errors::from_runner)?;
        Ok(runs.into_iter().find(|r| r.name == self.name).and_then(|r| r.provider).map(|p| isothrum_runner::wire_name(&p)))
    }

    /// Runs the lab's set-up again on its running machines (`machine`, or all): its provisioners,
    /// and its controller's play. The launch's values aren't kept, so none reaches it: an input's
    /// file keeps the value it was written with.
    pub(crate) async fn provision(&self, machine: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        self.act_placed(remote::Act::Provision(machine, None), log).await
    }

    /// Takes `machine` out of the running lab, ahead of its removal from the lab's files.
    pub(crate) async fn retire(&self, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.require_up()?;
        self.act_placed(remote::Act::Retire(machine), log).await
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<Status>> {
        if !self.files_back()? {
            return Ok(None);
        }
        self.read().await.map(Some)
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given.
    pub(crate) fn console_command(&self, machine: &str, user: Option<&str>) -> Result<ShellCommand> {
        self.lab()?.console_command(machine, user).map_err(errors::from_runner)
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with the run's
    /// options (an ESXi host's settings and password).
    pub(crate) async fn graphical_console(&self, machine: &str) -> Result<isothrum_runner::Console> {
        self.lab()?.graphical_console(machine, &self.placed(RunOptions::new())).await.map_err(errors::from_runner)
    }

    /// What opens running VM `machine` in its hypervisor's own app, as the runner says.
    pub(crate) async fn native_console(&self, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        self.lab()?.native_console(machine, &self.placed(RunOptions::new())).await.map_err(errors::from_runner)
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

/// Ends every tool cut short that is still stopping, at once: the app exiting now.
pub(crate) fn end_cut_short_tools() {
    PROCESSES.end_stopping();
}

/// A runner at `home` with this host's tools, a container lab's docker calls pinned to the engine
/// chosen at its launch ([`EngineProcesses`]), else the current Docker context as before.
fn host_config(home: &Path) -> RunnerConfig {
    let mut config = RunnerConfig::new(home);
    config.processes = Arc::new(EngineProcesses::new(PROCESSES.clone()));
    config
}

/// What a start hands the lab: the launch's values for the manifest's secret and input slots (a
/// value no slot takes is never delivered), and whether its services keep their own host ports
/// (`CTFd` on 8000) rather than free ones picked at random.
#[derive(Clone, Copy, Default)]
pub(crate) struct Inputs<'a> {
    /// The launch's values, by the name of the manifest slot each goes to.
    pub(crate) values: &'a [(String, String)],
    /// Whether its services keep their own host ports rather than free ones picked at random.
    pub(crate) default_ports: bool,
}

/// The values' names only: a value can be a launch token or a flag.
impl std::fmt::Debug for Inputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inputs")
            .field("names", &self.values.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .field("default_ports", &self.default_ports)
            .finish()
    }
}

#[cfg(test)]
impl<'a> Inputs<'a> {
    /// The launch's `values`, on host ports picked at random.
    pub(crate) fn values(values: &'a [(String, String)]) -> Self {
        Inputs { values, default_ports: false }
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
