//! Where the lab the box joins runs, and how the box goes in and out of it there: the box added
//! to (or taken out of) the lab's spec, the lab regenerated for its form, then the run brought up
//! to date with the regenerated lab, with what changing it needs (a cloud account's credentials,
//! a server's connection). The same on every form; only the run and its place differ.

use std::path::PathBuf;

use isoloom_spec::Target;

use super::{Cpu, Kind};
use crate::error::Result;
use crate::platform::AppDirs;
use crate::runtime::isothrum::AttackStatus;
use crate::runtime::{isothrum, lab, server};

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

impl Host {
    async fn update(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<()> {
        match self {
            Host::Local(run) => run.update(log).await,
            Host::Cloud(run, place) => run.update(place, log).await,
            Host::Server { run, host, conn } => run.update(&crate::runtime::launch::server_place(dirs, host, conn, run).await?, log).await,
        }
    }

    async fn retire(&self, dirs: &AppDirs, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        match self {
            Host::Local(run) => run.retire(machine, log).await,
            Host::Cloud(run, place) => run.retire(place, machine, log).await,
            Host::Server { run, host, conn } => run.retire(&crate::runtime::launch::server_place(dirs, host, conn, run).await?, machine, log).await,
        }
    }

    async fn status(&self, machine: &str, network: Option<&str>, vm: bool) -> Result<AttackStatus> {
        match self {
            Host::Local(run) => run.machine_box_status(machine, network).await,
            Host::Cloud(run, _) => run.machine_box_status(machine, network, vm).await,
            Host::Server { run, .. } => run.machine_box_status(machine, network, vm).await,
        }
    }

    fn console(&self, machine: &str) -> Result<isothrum_runner::ShellCommand> {
        match self {
            Host::Local(run) => run.console_command(machine),
            Host::Cloud(run, _) => run.console_command(machine),
            Host::Server { run, .. } => run.console_command(machine),
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

impl Placed {
    /// Adds the box onto `network` (else the lab's first machine's), from `image` beside
    /// containers, then brings the run up to date with it. A run that isn't up is refused before
    /// anything of it changes: the lab is put back as it was.
    pub(crate) async fn add(&self, dirs: &AppDirs, image: &str, network: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let kind = if self.vm { Kind::Vm } else { Kind::Container { image } };
        super::add(&self.dir, kind, self.cpu(), network)?;
        lab::prepare(&self.dir, self.target)?;
        match self.host.update(dirs, log).await {
            Err(e) if isothrum::errors::is_not_running(&e) => {
                super::remove(&self.dir)?;
                lab::prepare(&self.dir, self.target)?;
                Err(e)
            }
            done => done,
        }
    }

    /// Takes the box out: retired from the run (what the update wouldn't remove itself), out of
    /// the lab, then the run brought up to date. Nothing to do when the lab hasn't it.
    pub(crate) async fn remove(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<()> {
        if !super::added(&self.dir) {
            return Ok(());
        }
        self.host.retire(dirs, &super::name(&self.dir), log).await?;
        super::remove(&self.dir)?;
        lab::prepare(&self.dir, self.target)?;
        self.host.update(dirs, log).await
    }

    /// The box as the lab page shows it.
    pub(crate) async fn status(&self, network: Option<&str>) -> Result<AttackStatus> {
        self.host.status(&super::name(&self.dir), network, self.vm).await
    }

    /// The command that opens a shell into the box.
    pub(crate) fn shell_command(&self) -> Result<isothrum_runner::ShellCommand> {
        self.host.console(&super::name(&self.dir))
    }

    /// The box's CPU: this computer's own where the lab runs on it and it is Arm (a lab's
    /// machines are amd64 unless they say otherwise; the VM a container lab runs in here is this
    /// computer's own CPU too); the lab's on a server or in a cloud account.
    fn cpu(&self) -> Cpu {
        let native = matches!(self.target, Target::Docker | Target::DockerVm | Target::Vagrant);
        if self.local() && native && std::env::consts::ARCH == "aarch64" { Cpu::Arm } else { Cpu::Lab }
    }

    /// Whether the box runs here, on this machine (its image is the learner's local download).
    pub(crate) fn local(&self) -> bool {
        matches!(self.host, Host::Local(_))
    }
}
