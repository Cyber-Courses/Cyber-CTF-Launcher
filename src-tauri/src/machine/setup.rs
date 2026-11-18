//! The guided machine setup: what to install, start or restart on this machine, in which order,
//! for the lab types the player wants (containers, VMs), with which engine and which hypervisor.
//! The rules are isothrum-machine's ([`isothrum_machine::setup`]):
//! - the machine check carries, for each choice the setup window offers, what that choice takes
//!   here ([`Options`], the report's `setup`): the window renders those actions and runs each
//!   through its command, never working them out itself;
//! - [`plan`] puts them in order for a whole setup, from what is wanted, and `run::walk` runs it
//!   step by step: the command line's `machine setup` and the headless entry the test rig runs on
//!   a bare machine (`integration_tests::setup`); the window walks the same options one click at
//!   a time.
//!
//! The launcher's part is the window, the player's words for each reason, and the machines the
//! webview's tests render the window on (`shapes`).

pub(crate) mod run;
#[cfg(test)]
pub(crate) mod shapes;

pub use isothrum_machine::setup::{Action, Options, WindowsFeature};
#[cfg(test)]
pub(crate) use isothrum_machine::setup::{ManualReason, Step};
pub(crate) use isothrum_machine::setup::{PassedOver, Plan, SetupPath, Wanted};

use isothrum_machine::setup::{DockerFound, Machine};

use crate::machine::system::SystemReport;
#[cfg(test)]
use crate::machine::system::docker::Engine;
use crate::machine::tools::install::{Dependency, name};
use crate::runtime::providers::{self, Provider};
use isothrum_machine::{Arch, HostOs, LinuxPackages, Packages};

/// The CPU a report names by Rust's name for it ([`crate::platform::cpu_name`]).
pub(crate) fn arch_of(name: &str) -> Arch {
    if name == crate::platform::cpu_name(Arch::Arm64) { Arch::Arm64 } else { Arch::Amd64 }
}

/// The computer the machine `r` reports: this one, as the report names its OS, CPU and package
/// manager.
pub(crate) fn host_of(r: &SystemReport) -> isothrum_machine::Host {
    let mut host = providers::machine().host;
    host.os = r.os;
    // The CPU and the package manager the report says, which decide what an install asks (an
    // Intel Mac's long builds, Fedora's VirtualBox group).
    host.arch = arch_of(r.arch);
    if r.os == HostOs::Linux {
        host.packages = match r.pkg_manager.name.as_str() {
            "apt" => Some(Packages::Linux(LinuxPackages::Apt)),
            "dnf" => Some(Packages::Linux(LinuxPackages::Dnf)),
            _ => None,
        };
    }
    host
}

/// What the setup reads of the machine `r` reports.
fn machine(r: &SystemReport) -> Machine {
    Machine {
        host: host_of(r),
        packages_installed: r.pkg_manager.installed,
        docker: DockerFound {
            installed: r.docker.installed,
            running: r.docker_running,
            denied: r.docker_denied,
            engine: r.docker_engine,
            engines_running: r.docker_engines_running.clone(),
        },
        wsl: r.wsl.as_ref().map(|w| w.state),
        providers: r.vm_providers.clone(),
        groups_pending: r.groups_pending.clone(),
        elevation: crate::machine::tools::install::elevation(),
        user: crate::platform::exec::login_name(),
    }
}

/// The engines the setup offers on `os`, in the order it shows them.
#[cfg(test)]
pub fn engines_on(os: HostOs) -> &'static [Engine] {
    isothrum_machine::setup::engines_on(os)
}

/// The engine the setup installs in one click on `os`.
#[cfg(test)]
pub fn recommended_engine(os: HostOs) -> Engine {
    isothrum_machine::setup::recommended_engine(os)
}

/// Docker answers: its CLI is installed and its daemon runs for this account.
#[cfg(test)]
pub fn docker_ready(r: &SystemReport) -> bool {
    r.docker.installed && r.docker_running
}

/// The hypervisor the setup installs as `provider` on `os`, when it installs it.
#[cfg(test)]
pub(crate) fn installable(provider: Provider, os: HostOs) -> Option<Dependency> {
    isothrum_machine::setup::installable(provider.vagrant()?, os)
}

/// What each choice takes on the machine `r` reports; `hyperv_restart`: the setup turned Hyper-V
/// on in this boot, so it waits for a restart.
pub fn options(r: &SystemReport, hyperv_restart: bool) -> Options {
    isothrum_machine::setup::options(&machine(r), hyperv_restart)
}

/// The whole setup `wanted` on the machine `r` reports, with its `options` (the report's `setup`).
pub fn plan(r: &SystemReport, options: &Options, wanted: &Wanted) -> Plan {
    isothrum_machine::setup::plan(&machine(r), options, wanted)
}

/// Why a wanted hypervisor isn't the one the setup sets up, as the command line and the test rig
/// say it.
pub fn passed_over_said(provider: isothrum_machine::providers::Provider, why: &PassedOver) -> String {
    match why {
        PassedOver::LinksOut { url } => format!("links out: {}", url.as_deref().unwrap_or("its vendor's site")),
        PassedOver::Distribution => {
            format!("this distribution isn't supported by the guided setup yet: install {} yourself", Provider::from(provider).label())
        }
        PassedOver::CantRun { reason } => format!("can't run here: {}", providers::said(reason)),
        PassedOver::NotOffered => "not offered on this machine".to_owned(),
        PassedOver::NotNeeded { chosen } => format!("not needed: {} is set up", Provider::from(*chosen).label()),
        _ => format!("{why:?}"),
    }
}

/// Whether `dependency`'s install is left to the player here, as the reason words it.
pub(crate) fn distribution_said(dependency: Option<Dependency>) -> String {
    format!("this distribution isn't supported by the guided setup yet: install {} yourself", dependency.map_or("it", name))
}

#[cfg(test)]
mod tests;
