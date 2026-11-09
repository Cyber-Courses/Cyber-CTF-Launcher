//! Where the lab the box joins runs, and how the box goes in and out of it there: the box added
//! to (or taken out of) the lab's spec, the lab regenerated for its form, then the run brought up
//! to date with the regenerated lab, with what changing it needs (a cloud account's credentials,
//! a server's connection). The same on every form; only the run and its place differ.

use std::path::{Path, PathBuf};

use isoloom_spec::Target;
use isothrum_runner::Status;

use super::status::{AttackStatus, display, status_of};
use super::{Cpu, Kind};
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::paths::{local_runtime, local_vm, runner_home};
use crate::runtime::providers::Provider;
use crate::runtime::{Runtime, isothrum, lab, server};

/// The run holding the lab, and what changing it needs.
pub(crate) enum Host {
    /// On this machine: on Docker, in one local VM, or as VMs.
    Local(isothrum::LabRun),
    /// In a cloud account, with its credentials.
    Cloud(isothrum::CloudRun, isothrum::CloudPlace),
    /// On a Proxmox server (`host`, the launcher's id for it), through `conn`: its place is
    /// found when the run is changed (its node, its SSH jump), not to read the box's status.
    Server { run: isothrum::ServerRun, host: String, conn: server::Connection },
}

/// Why changing the run failed, and whether the run may have changed by then.
struct Failed {
    error: Error,
    /// False only when the run certainly didn't change: its place couldn't be reached, or the
    /// runner refused before acting ([`isothrum::errors::refused_before_acting`]).
    run_changed: bool,
}

impl Failed {
    fn of_runner(error: Error) -> Self {
        let run_changed = !isothrum::errors::refused_before_acting(&error);
        Failed { error, run_changed }
    }
}

impl Host {
    async fn update(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> std::result::Result<(), Failed> {
        match self {
            Host::Local(run) => run.update(log).await.map_err(Failed::of_runner),
            Host::Cloud(run, place) => run.update(place, log).await.map_err(Failed::of_runner),
            Host::Server { run, host, conn } => {
                let place = crate::runtime::launch::server_place(dirs, host, conn, run).await.map_err(|error| Failed { error, run_changed: false })?;
                run.update(&place, log).await.map_err(Failed::of_runner)
            }
        }
    }

    async fn retire(&self, dirs: &AppDirs, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        match self {
            Host::Local(run) => run.retire(machine, log).await,
            Host::Cloud(run, place) => run.retire(place, machine, log).await,
            Host::Server { run, host, conn } => run.retire(&crate::runtime::launch::server_place(dirs, host, conn, run).await?, machine, log).await,
        }
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    async fn run_status(&self) -> Result<Option<Status>> {
        match self {
            Host::Local(run) => run.run_status().await,
            Host::Cloud(run, _) => run.run_status().await,
            Host::Server { run, .. } => run.run_status().await,
        }
    }

    fn console(&self, machine: &str) -> Result<isothrum_runner::ShellCommand> {
        match self {
            Host::Local(run) => run.console_command(machine),
            Host::Cloud(run, _) => run.console_command(machine),
            Host::Server { run, .. } => run.console_command(machine),
        }
    }

    /// Runs the lab's checks where it runs (a cloud's and a server's over SSH).
    pub(crate) async fn check(&self) -> Result<crate::runtime::docker::Check> {
        match self {
            Host::Local(run) => run.check().await,
            Host::Cloud(run, place) => run.check(place).await,
            Host::Server { run, .. } => run.check().await,
        }
    }
}

/// The lab the box joins: its run (`host`), its folder, the Isoloom target it is generated for,
/// and whether its machines are VMs (the box is Kali then) or containers (the picked image).
pub(crate) struct Placed {
    pub(crate) host: Host,
    pub(crate) dir: PathBuf,
    pub(crate) target: Target,
    pub(crate) vm: bool,
}

/// Where lab `id`, installed in `dir` and declared `runtime`, runs, in the form it was started in
/// (a container lab started as one VM per machine is VMs): its run on this machine, on a Proxmox
/// server or in a cloud account. `None` where the launcher reaches no run of it: on an ESXi host
/// (its runs are the launcher's own, see `launch::esxi_run`), or a cloud-services lab here.
pub(crate) fn locate(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<Option<Placed>> {
    let home = runner_home(dirs)?;
    let form = local_runtime(dir, runtime);
    let (host, target) = match server::lab_connection(dirs, dir)? {
        Some(c) if c.provider == Provider::VmwareEsxi => return Ok(None),
        Some(c) if c.provider.is_cloud() => {
            let cloud = server::terraform_target(c.provider).unwrap_or_default();
            let place = isothrum::CloudPlace {
                label: server::marked_host(dir).unwrap_or_default(),
                region: c.region.clone(),
                tf_env: c.tf_env.clone(),
                ..isothrum::CloudPlace::default()
            };
            let run = isothrum::CloudRun::new(&home, dir, id, form, cloud)?;
            (Host::Cloud(run, place), lab::terraform(dir, form, cloud).1)
        }
        Some(conn) => {
            let run = isothrum::ServerRun::new(&home, dir, id, form)?;
            let host = server::marked_host(dir).unwrap_or_default();
            (Host::Server { run, host, conn }, lab::terraform(dir, form, "proxmox").1)
        }
        None => {
            let (run, target) = match form {
                Runtime::Docker if local_vm(dir).is_some() => (isothrum::LabRun::docker_in_vm(&home, dir, id)?, lab::vagrant_target(Runtime::Docker)),
                Runtime::Docker => (isothrum::LabRun::docker(&home, dir, id)?, Target::Docker),
                Runtime::Vm => (isothrum::LabRun::vm(&home, dir, id)?, Target::Vagrant),
                Runtime::Cloud => return Ok(None),
            };
            (Host::Local(run), target)
        }
    };
    Ok(Some(Placed { host, dir: dir.to_path_buf(), target, vm: form == Runtime::Vm }))
}

impl Placed {
    /// Adds the box onto `network` (else the lab's first machine's), from `image` beside
    /// containers, then brings the run up to date with it.
    ///
    /// A failure before the run could have changed puts the lab back as it was (the box out of
    /// its spec, its files regenerated): the box's spec or the lab's files couldn't be written,
    /// the run's place couldn't be reached, or the runner refused before acting (the run isn't
    /// up, another operation holds it). A failure while the runner changed the run keeps the box
    /// in the lab, so the learner's Stop retires whatever of it the run already has.
    pub(crate) async fn add(&self, dirs: &AppDirs, image: &str, network: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let kind = if self.vm { Kind::Vm } else { Kind::Container { image } };
        let written = super::add(&self.dir, kind, self.cpu(), network).and_then(|()| lab::prepare(&self.dir, self.target).map(drop));
        let failed = match written {
            Err(error) => Failed { error, run_changed: false },
            Ok(()) => match self.host.update(dirs, log).await {
                Ok(()) => return Ok(()),
                Err(failed) => failed,
            },
        };
        if !failed.run_changed
            && let Err(e) = self.undo_add()
        {
            log::warn!("couldn't put the lab back as it was after a failed attack box add: {e}");
        }
        Err(failed.error)
    }

    /// The lab as it was before an add: the box out of its spec, its files regenerated.
    fn undo_add(&self) -> Result<()> {
        super::remove(&self.dir)?;
        lab::prepare(&self.dir, self.target).map(drop)
    }

    /// Takes the box out: retired from the run (what the update wouldn't remove itself), out of
    /// the lab, then the run brought up to date. Nothing to do when the lab hasn't it.
    pub(crate) async fn remove(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<()> {
        if !super::added(&self.dir)? {
            return Ok(());
        }
        self.host.retire(dirs, &super::name(&self.dir)?, log).await?;
        super::remove(&self.dir)?;
        lab::prepare(&self.dir, self.target)?;
        self.host.update(dirs, log).await.map_err(|f| f.error)
    }

    /// The box as the lab page shows it: read by its name from the run's status (down, with no
    /// shell, before the lab's files exist).
    pub(crate) async fn status(&self, network: Option<&str>) -> Result<AttackStatus> {
        let Some(status) = self.host.run_status().await? else {
            return Ok(AttackStatus { vm: self.vm, ..AttackStatus::default() });
        };
        let name = super::name(&self.dir)?;
        let shell = self.host.console(&name).ok().map(|c| display(&c)).unwrap_or_default();
        Ok(status_of(&status, &name, network, shell, self.vm))
    }

    /// The command that opens a shell into the box.
    pub(crate) fn shell_command(&self) -> Result<isothrum_runner::ShellCommand> {
        self.host.console(&super::name(&self.dir)?)
    }

    /// The box's CPU on this computer ([`box_cpu`]).
    fn cpu(&self) -> Cpu {
        box_cpu(std::env::consts::ARCH, self.local(), self.target)
    }

    /// Whether the box runs here, on this machine (its image is the learner's local download).
    pub(crate) fn local(&self) -> bool {
        matches!(self.host, Host::Local(_))
    }
}

/// The box's CPU for a lab of `target`, run on this computer (`local`) or not, `arch` being this
/// computer's (`std::env::consts::ARCH`): this computer's own where the lab runs on it and it is
/// Arm (a lab's machines are amd64 unless they say otherwise; the VM a container lab runs in here
/// is this computer's CPU too), whatever the OS; the lab's on a server or in a cloud account, and
/// on an x86 computer.
fn box_cpu(arch: &str, local: bool, target: Target) -> Cpu {
    let native = matches!(target, Target::Docker | Target::DockerVm | Target::Vagrant);
    if local && native && arch == "aarch64" { Cpu::Arm } else { Cpu::Lab }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every computer the launcher ships for, as Rust names its CPU: Apple Silicon and Intel Macs,
    /// Windows on x64 and Arm, Linux on x64 and Arm.
    const ARM: &str = "aarch64";
    const X86: &str = "x86_64";

    #[test]
    fn on_an_arm_computer_the_box_of_a_lab_here_runs_natively_on_every_local_form() {
        for target in [Target::Docker, Target::DockerVm, Target::Vagrant] {
            assert_eq!(box_cpu(ARM, true, target), Cpu::Arm, "{target:?}");
        }
    }

    #[test]
    fn on_an_x86_computer_the_box_takes_the_labs_cpu() {
        for target in [Target::Docker, Target::DockerVm, Target::Vagrant] {
            assert_eq!(box_cpu(X86, true, target), Cpu::Lab, "{target:?}");
        }
    }

    #[test]
    fn on_a_server_or_in_a_cloud_account_the_box_takes_the_labs_cpu_whatever_this_computer_is() {
        for arch in [ARM, X86] {
            for target in [Target::DockerVm, Target::Proxmox, Target::CloudDocker, Target::CloudVm] {
                assert_eq!(box_cpu(arch, false, target), Cpu::Lab, "{arch} {target:?}");
            }
        }
    }
}
