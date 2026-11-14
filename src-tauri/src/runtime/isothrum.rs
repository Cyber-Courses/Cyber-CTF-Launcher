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
//! launcher always used), the runner's home (the app data folder, so runs are kept under its
//! `runs/`), the words the player reads (the runner's events in `progress`, its error codes in
//! `errors`) and the lab page's shapes (`view`). The attack box is no concern of the run's: the
//! launcher adds it to the lab as one of its machines (`attack_box`), and the run is brought
//! up to date with it ([`LabRun::update`], [`LabRun::retire`]) like with any change of the lab.
//! An operation that outlives the app runs in the launcher's own deploy worker, which calls the runner in-process;
//! stopping that worker drops the operation, which ends what it started.

mod cloud_runs;
mod described;
pub(crate) mod errors;
mod gone;
mod one_read;
mod opened;
mod progress;
mod server;
mod view;

pub(crate) use cloud_runs::{CloudPlace, CloudRun, reap_expired as reap_expired_clouds};
pub(crate) use server::{ServerPlace, ServerRun};

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

use isothrum_runner::{
    BoxFuture, Format, ProcessRunner, ProcessSpec, RunName, RunOptions, Runner, RunnerConfig, Running, SecretValue, Secrets, ShellCommand, Status,
    SystemProcesses,
};
use isothrum_runner::{Exit, OnLine, ProcessError};

use super::docker::Check;
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

/// Every run the runner whose home is `home` records, each in the state its last operation left
/// it in, as its store says.
pub(crate) fn records(home: &Path) -> Result<Vec<isothrum_runner::RunRecord>> {
    Runner::new(host_config(home, None)).map_err(errors::from_runner)?.runs().map_err(errors::from_runner)
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

/// Where a VM lab on an ESXi server runs (Vagrant's `vmware_esxi`): the server's id, so an
/// operation asked for elsewhere is refused; its settings as the manifest's params; its password,
/// given to every operation of the run (status and park too). The runner relays the host's SSH
/// and HTTPS itself where this machine can't reach it (macOS).
#[derive(Clone)]
pub(crate) struct EsxiPlace {
    /// The server's id.
    pub(crate) label: String,
    /// The server's name, as Servers shows it.
    pub(crate) name: String,
    /// The host's settings, by the manifest's param names.
    pub(crate) params: Vec<(String, String)>,
    password: SecretValue,
}

impl std::fmt::Debug for EsxiPlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EsxiPlace").field("label", &self.label).field("params", &self.params).finish_non_exhaustive()
    }
}

impl EsxiPlace {
    /// Server `label`'s place (`name` in Servers), from the environment the launcher keeps for its
    /// labs (`lab::esxi_env`'s names): the settings the manifest takes, and the password.
    pub(crate) fn from_env(label: &str, name: &str, env: &[(String, String)]) -> EsxiPlace {
        let value = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        EsxiPlace {
            label: label.to_owned(),
            name: name.to_owned(),
            params: lab::esxi::PARAMS.iter().filter_map(|name| value(name).map(|v| ((*name).to_owned(), v))).collect(),
            password: SecretValue::new(value(lab::esxi::PASSWORD).unwrap_or_default()),
        }
    }

    /// `opts` on this host: its id recorded, its settings as environment, its password among
    /// the secrets.
    pub(crate) fn onto(&self, opts: RunOptions) -> RunOptions {
        let mut opts = self.params.iter().fold(opts.with_host_label(self.label.clone()), |o, (k, v)| o.with_env(k.clone(), v.clone()));
        opts.secrets.insert(lab::esxi::PASSWORD, self.password.clone());
        opts
    }
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
}

impl LabRun {
    /// Container lab `id`, installed in `dir`, on the runner whose home is `home` (the app data
    /// folder), with this host's tools.
    pub(crate) fn docker(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        // The lab's docker calls go to the engine chosen at launch (its marker), if any.
        LabRun::with(host_config(home, paths::engine(dir)), dir, id, Kind::Docker)
    }

    /// VM lab `id`, installed in `dir`, on the runner whose home is `home`, with this host's tools.
    pub(crate) fn vm(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::with(host_config(home, None), dir, id, Kind::Vm)
    }

    /// Container lab `id` left by an earlier version in one local helper VM (Isoloom's former
    /// `docker-vm`: Docker Compose inside a VM), installed in `dir`, on the runner whose home is
    /// `home`: to read, stop and remove that run.
    pub(crate) fn docker_in_vm(home: &Path, dir: &Path, id: &str) -> Result<LabRun> {
        LabRun::with(host_config(home, None), dir, id, Kind::DockerVm)
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
        Ok(LabRun::with(host_config(home, None), dir, id, kind)?.on(place))
    }

    /// The run on the ESXi server `place`.
    fn on(self, place: EsxiPlace) -> LabRun {
        LabRun { esxi: Some(place), ..self }
    }

    /// A run on a runner built from `config` (tests script its tools).
    fn with(config: RunnerConfig, dir: &Path, id: &str, kind: Kind) -> Result<LabRun> {
        let name = RunName::new(&super::docker::project(id)).map_err(errors::from_runner)?;
        let runner = Runner::new(config).map_err(errors::from_runner)?;
        Ok(LabRun { runner, dir: dir.to_path_buf(), name, kind, esxi: None })
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
    async fn lab(&self) -> Result<Running<'_>> {
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
        up_with(&self.lab().await?, &self.placed(start_options(inputs, provider)), log).await.map_err(|e| self.said(e))?;
        let message = self.target().and_then(|t| described::message(&self.dir, t));
        message.iter().flat_map(|m| m.lines()).for_each(|l| log(l.to_owned()));
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
        let (lab, opts) = (&self.lab().await?, &self.placed(RunOptions::new()));
        when_unlocked(log, move |sink| async move { lab.down(opts, &sink).await }).await.map_err(|e| self.said(errors::from_runner(e)))
    }

    /// Parks the lab, keeping what it created: Docker only stops containers, so both of the
    /// launcher's park modes are the manifest's `shutdown` there; VMs are suspended (`Pause`) or
    /// powered off (`Shutdown`).
    pub(crate) async fn park(&self, mode: Park, log: &mut impl FnMut(String)) -> Result<()> {
        let (lab, opts) = (&self.lab().await?, &self.placed(RunOptions::new()));
        let mode = match (self.kind, mode) {
            // VMs (a VM lab, or the VM a container lab runs in) suspend on Pause; Docker only
            // stops its containers, so both of its park modes are the manifest's `shutdown`.
            (Kind::Vm | Kind::DockerVm, Park::Pause) => isothrum_runner::Park::Pause,
            _ => isothrum_runner::Park::Shutdown,
        };
        when_unlocked(log, move |sink| async move { lab.park(mode, opts, &sink).await }).await.map_err(|e| self.said(errors::from_runner(e)))
    }

    /// Starts the parked lab again. When nothing is left of it, the launcher's parked mark goes
    /// too, so the lab page offers a fresh start instead of a resume that can't work.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        let (lab, opts) = (&self.lab().await?, &self.placed(RunOptions::new()));
        match when_unlocked(log, move |sink| async move { lab.resume(opts, &sink).await }).await {
            Ok(_) => Ok(()),
            Err(e) if e.code() == isothrum_runner::ErrorCode::NothingToResume => {
                paths::mark_parked(&self.dir, None)?;
                Err(errors::from_runner(e))
            }
            Err(e) => Err(self.said(errors::from_runner(e))),
        }
    }

    /// The lab as its page shows it; not running before its files were ever generated.
    pub(crate) async fn status(&self) -> Result<LabStatus> {
        let place = if matches!(self.kind, Kind::Vm | Kind::DockerVm) { Place::LocalVm } else { Place::Container };
        if !self.files_back()? {
            return Ok(LabStatus { place: Some(place), ..LabStatus::default() });
        }
        let lab = self.lab().await?;
        let status = self.read(&lab).await?;
        Ok(LabStatus { place: Some(place), ..view::lab_status(&self.described()?, &status, paths::parked(&self.dir)) })
    }

    /// `lab`'s status, read by the runner, one read of the run at a time in the app
    /// ([`one_read`]): the lab page's and the attack box's never ask Vagrant at once.
    async fn read(&self, lab: &Running<'_>) -> Result<Status> {
        one_read::status(&self.dir.join(self.name.as_str()), || async { lab.status().await.map_err(|e| self.said(errors::from_runner(e))) }).await
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
        let (lab, opts) = (&self.lab().await?, &self.placed(RunOptions::new()));
        when_unlocked(log, move |sink| async move { lab.provision(machine, opts, &sink).await }).await.map_err(|e| self.said(errors::from_runner(e)))
    }

    /// Runs the lab's checks; not available when it declares none (nothing runs then).
    pub(crate) async fn check(&self) -> Result<Check> {
        checks_of(&self.lab().await?, &self.described()?.check_runners, self.kind != Kind::Docker, &self.placed(RunOptions::new())).await
    }

    /// Brings the running lab up to date with its regenerated files (a machine added to the lab
    /// or taken out), with the parameters it was started with. A VM restarted for its new
    /// settings boots like a started one, into the same passing UTM refusal: see
    /// [`LabRun::after_utm_refusal`].
    pub(crate) async fn update(&self, log: &mut impl FnMut(String)) -> Result<()> {
        // Boxed: each try is a whole run's operation, a large future.
        match Box::pin(self.attempt(Op::Update, log)).await {
            Err(e) => Box::pin(self.after_utm_refusal(Op::Update, e, log)).await,
            done => done,
        }
    }

    /// [`LabRun::up`] on `provider`; on UTM, a start its passing scripting refusal cut short goes
    /// on as [`LabRun::after_utm_refusal`] says.
    pub(crate) async fn start(&self, inputs: Inputs<'_>, provider: &str, log: &mut impl FnMut(String)) -> Result<()> {
        let op = Op::Up { inputs, provider };
        match Box::pin(self.attempt(op, log)).await {
            Err(e) if provider == super::providers::Provider::Utm.id() => Box::pin(self.after_utm_refusal(op, e, log)).await,
            done => done,
        }
    }

    /// `op`, once.
    async fn attempt(&self, op: Op<'_>, log: &mut impl FnMut(String)) -> Result<()> {
        match op {
            Op::Up { inputs, provider } => self.up(inputs, Some(provider), log).await,
            Op::Update => update_of(&self.lab().await?, &self.placed(RunOptions::new()), log).await,
        }
    }

    /// What follows `op` that failed with `failed`: when that is UTM's passing scripting refusal
    /// ([`errors::try_again_after_utm`], said in `log`), one more try, else the error. The try is
    /// `op` again while the run still runs; when the failed attempt left it down (a restart for
    /// new settings shut the VM down before UTM refused its boot), an update would be refused
    /// ("isn't running"), so the try is an `up` on the current files, on the run's provider, which
    /// brings the run back with what `op` was adding.
    async fn after_utm_refusal(&self, op: Op<'_>, failed: Error, log: &mut impl FnMut(String)) -> Result<()> {
        if !errors::try_again_after_utm(&failed, log) {
            return Err(failed);
        }
        let Some(provider) = self.left_down().await else { return self.attempt(op, log).await };
        log("The run isn't running after that: starting it again on its current files.".into());
        match op {
            Op::Up { inputs, provider } => self.up(inputs, Some(provider), log).await,
            // An update carries no launch's values, and neither does the start that replaces it;
            // a run whose status names no provider is on the one its lab folder records.
            Op::Update => self.up(Inputs::default(), provider.or_else(|| paths::local_vm(&self.dir)).as_deref(), log).await,
        }
    }

    /// The run's provider when the run is down (nothing of it running), as its status says; none
    /// when it runs. A status that can't be read is logged, and taken for a run that runs.
    async fn left_down(&self) -> Option<Option<String>> {
        match self.run_status().await {
            Ok(Some(status)) if !view::runs(&status) => Some(status.provider.map(|p| isothrum_runner::wire_name(&p))),
            Ok(_) => None,
            Err(e) => {
                log::warn!("couldn't read the run's status after UTM's refusal ({e}): trying the same operation again");
                None
            }
        }
    }

    /// Takes `machine` out of the running lab, ahead of its removal from the lab's files.
    pub(crate) async fn retire(&self, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        retire_from(&self.lab().await?, machine, &self.placed(RunOptions::new()), log).await
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<Status>> {
        if !self.files_back()? {
            return Ok(None);
        }
        let lab = self.lab().await?;
        self.read(&lab).await.map(Some)
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given.
    pub(crate) async fn console_command(&self, machine: &str, user: Option<&str>) -> Result<ShellCommand> {
        self.lab().await?.console_command(machine, user).map_err(errors::from_runner)
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with the run's
    /// options (an ESXi host's settings and password).
    pub(crate) async fn graphical_console(&self, machine: &str) -> Result<isothrum_runner::Console> {
        self.lab().await?.graphical_console(machine, &self.placed(RunOptions::new())).await.map_err(errors::from_runner)
    }

    /// What opens running VM `machine` in its hypervisor's own app, as the runner says.
    pub(crate) async fn native_console(&self, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        self.lab().await?.native_console(machine, &self.placed(RunOptions::new())).await.map_err(errors::from_runner)
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

/// An operation on a run that UTM's passing refusal may cut short ([`LabRun::after_utm_refusal`]).
#[derive(Clone, Copy)]
enum Op<'a> {
    /// A start with the launch's `inputs`, on `provider`.
    Up { inputs: Inputs<'a>, provider: &'a str },
    /// Bringing the running run up to date with its files.
    Update,
}

/// Brings `lab` up to date with its regenerated manifest, with `opts`, waiting out another
/// operation's hold on the run.
async fn update_of(lab: &Running<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
    when_unlocked(log, move |sink| async move { lab.update(opts, &sink).await }).await.map_err(errors::from_runner)
}

/// Takes `machine` out of `lab` ahead of its removal from the manifest, with `opts`, waiting out
/// another operation's hold on the run.
async fn retire_from(lab: &Running<'_>, machine: &str, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
    when_unlocked(log, move |sink| async move { lab.retire(machine, opts, &sink).await }).await.map_err(errors::from_runner)
}

/// Brings `lab` up with `opts`, its events as `log`'s lines.
async fn up_with(lab: &Running<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
    let (sink, lines) = progress::channel();
    progress::follow(log, lines, lab.up(opts, &sink)).await.map(drop).map_err(errors::from_runner)
}

/// Runs `lab`'s checks in each of its `runners` (Compose services, or VMs when `vms`, where
/// Isoloom installed its runner script); not available when it has none (nothing runs then).
async fn checks_of(lab: &Running<'_>, runners: &[String], vms: bool, opts: &RunOptions) -> Result<Check> {
    if runners.is_empty() {
        return Ok(Check::default());
    }
    let argv = if vms { described::vagrant_check_command() } else { Vec::new() };
    let (mut lines, mut all_ok) = (Vec::new(), true);
    for runner in runners {
        let (sink, events) = progress::channel();
        let mut said = Vec::new();
        let report = progress::follow(&mut |l| said.push(l), events, lab.exec(runner, &argv, opts, &sink)).await.map_err(errors::from_runner)?;
        let printed = report.stdout.lines().chain(report.stderr.lines()).map(str::to_owned);
        lines.extend(said.into_iter().chain(printed).map(|l| (runner.clone(), l)));
        all_ok &= report.exit == 0;
    }
    Ok(view::check(&lines, all_ok))
}

/// Pins a lab's docker/compose calls to one engine: when a Docker context was chosen for the run,
/// `docker …` becomes `docker --context <ctx> …`. The runner's `SystemProcesses` strips a
/// `DOCKER_CONTEXT` env (to follow the current context), so the flag is used instead; it takes
/// precedence and is not stripped. Non-docker tools, and docker calls that already carry
/// `--context`, pass through untouched.
struct EngineProcesses {
    inner: SystemProcesses,
    context: String,
}

impl ProcessRunner for EngineProcesses {
    fn run<'a>(&'a self, spec: &'a ProcessSpec, on_line: OnLine<'a>) -> BoxFuture<'a, std::result::Result<Exit, ProcessError>> {
        match pin_docker(spec, &self.context) {
            None => self.inner.run(spec, on_line),
            Some(pinned) => Box::pin(async move { self.inner.run(&pinned, on_line).await }),
        }
    }
}

/// `spec` with `--context <context>` put before its arguments, when it is a plain `docker` call;
/// None when it needs no change (not docker, or already carrying `--context`).
fn pin_docker(spec: &ProcessSpec, context: &str) -> Option<ProcessSpec> {
    if spec.program != "docker" || spec.args.first().is_some_and(|a| a == "--context") {
        return None;
    }
    let mut pinned = spec.clone();
    pinned.args.splice(0..0, ["--context".to_string(), context.to_string()]);
    Some(pinned)
}

/// The tools every run of this process starts, shared: a tool cut short (its operation dropped,
/// a cancel) is asked to stop on Unix and given a grace before it is ended, and this process waits
/// for those still stopping before it exits (`stop_cut_short_tools`, `end_cut_short_tools`).
static PROCESSES: std::sync::LazyLock<SystemProcesses> = std::sync::LazyLock::new(SystemProcesses::default);

/// Waits, at most `within`, for every tool cut short to stop on its own, then ends what is left:
/// before a process exits right after cancelling an operation (a stopped deploy worker), so no
/// tool outlives it. Unix only: on Windows a tool cut short is ended at once.
#[cfg(unix)]
pub(crate) async fn stop_cut_short_tools(within: std::time::Duration) {
    if tokio::time::timeout(within, PROCESSES.stopped()).await.is_err() {
        log::warn!("tools cut short were still stopping after {} s: ending them", within.as_secs());
        PROCESSES.end_stopping();
    }
}

/// Ends every tool cut short that is still stopping, at once: the app exiting now.
pub(crate) fn end_cut_short_tools() {
    PROCESSES.end_stopping();
}

/// A runner at `home` with this host's tools; a container lab's docker calls pinned to `context`
/// (the engine chosen at launch), else the current Docker context as before.
fn host_config(home: &Path, context: Option<String>) -> RunnerConfig {
    let processes = PROCESSES.clone();
    let runner: Arc<dyn ProcessRunner> = match context {
        Some(context) => Arc::new(EngineProcesses { inner: processes, context }),
        None => Arc::new(processes),
    };
    let mut config = RunnerConfig::new(home);
    config.processes = runner;
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
mod engine_tests {
    use super::{ProcessSpec, pin_docker};

    #[test]
    fn a_chosen_engine_pins_the_labs_docker_calls_through_its_context() {
        // Every docker/compose call the runner makes for the lab gets `--context <chosen>` first,
        // so it reaches the chosen engine however the current context changes under it.
        let pinned = pin_docker(&ProcessSpec::new("docker", ["compose", "-p", "cyberctf-x", "up"]), "orbstack").unwrap();
        assert_eq!(pinned.program, "docker");
        assert_eq!(pinned.args, ["--context", "orbstack", "compose", "-p", "cyberctf-x", "up"]);
        // A non-docker tool (vagrant for a VM form) is left exactly as it was.
        assert!(pin_docker(&ProcessSpec::new("vagrant", ["up"]), "orbstack").is_none());
        // A docker call that already names a context is left alone (never two `--context`).
        assert!(pin_docker(&ProcessSpec::new("docker", ["--context", "colima", "ps"]), "orbstack").is_none());
    }
}

#[cfg(test)]
mod test_lab;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vm_tests;
