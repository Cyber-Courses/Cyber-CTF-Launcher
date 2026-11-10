//! The guided machine setup's plan: what to install, start or restart on this machine, in which
//! order, for the lab types the player wants (containers, VMs), with which engine and which
//! hypervisor. The one place these rules live:
//! - the machine check carries, for each choice the setup window offers, what that choice takes
//!   here ([`Options`], the report's `setup`): the window renders those actions and runs each
//!   through its command, never working them out itself;
//! - `plan` (in `setup/plan.rs`) puts them in order for a whole setup, from what is wanted: the
//!   headless entry the test rig runs on a bare machine (`integration_tests::setup`, built with
//!   the library's tests, its only caller: the window walks the same options step by step).

#[cfg(test)]
mod plan;
#[cfg(test)]
pub(crate) mod shapes;

#[cfg(test)]
pub(crate) use plan::{Plan, SetupPath, Step, Wanted, plan};

use serde::Serialize;
use ts_rs::TS;

use crate::machine::system::SystemReport;
use crate::machine::system::docker::Engine;
use crate::machine::system::wsl::WslState;
use crate::machine::tools::install::Dependency;
use crate::runtime::providers::{Need, Provider, ProviderStatus};

/// One thing the setup does, or leaves to the player, on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(rename = "SetupAction")]
pub enum Action {
    /// Installs `dependency` (`install_dependency`): Docker, WSL 2, Vagrant, a hypervisor, the
    /// Vagrant VMware Utility.
    Install { dependency: Dependency },
    /// Installs the Vagrant plugin `plugin` (`install_vagrant_plugin`).
    Plugin { plugin: String },
    /// Starts the installed engine `engine` (`docker_start_engine`).
    StartEngine { engine: Engine },
    /// Points Docker at `engine`, which runs already (`docker_use_engine`).
    UseEngine { engine: Engine },
    /// Restarts the computer, which finishes turning `feature` on (`machine_restart`).
    Restart { feature: WindowsFeature },
    /// Signs out and back in: a group the player was added to (Linux's `docker`, `libvirt` and
    /// `kvm`) applies from the next login.
    Relogin,
    /// Not something the setup does here: the player gets it (`dependency`, when it is one the
    /// setup installs elsewhere; from `url`, when there is one), then checks the machine again.
    Manual { reason: ManualReason, dependency: Option<Dependency>, url: Option<String> },
    /// Installed, but it can't run here, and nothing the setup installs mends that: `reason`.
    Blocked { reason: String },
}

/// A Windows feature that starts only after a restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WindowsFeature {
    Wsl,
    Hyperv,
}

/// Why something is the player's to get.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ManualReason {
    /// From its vendor (behind a login or a licence): the setup links out.
    Vendor,
    /// The guided setup doesn't install on this Linux distribution yet (it installs through apt).
    Distribution,
}

/// What each choice of the setup window takes on this machine (part of the machine check).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SetupOptions")]
pub struct Options {
    /// What turns WSL 2 on (Windows): nothing once it is ready, or elsewhere.
    pub wsl: Vec<Action>,
    /// The engine the setup installs in one click on this OS.
    pub recommended_engine: Engine,
    /// The engine the setup works on until the player picks one: the one in use, else the
    /// recommended one.
    pub engine: Engine,
    /// The engines offered on this OS, in the order shown, each with what makes it the one Docker
    /// uses here (nothing: it is).
    pub engines: Vec<EngineOption>,
    /// The hypervisor the setup works on until the player picks one: an installed one that can
    /// run labs, else an installed one, else the first the setup installs, else the first.
    pub hypervisor: Option<Provider>,
    /// The local hypervisors offered here, in the machine check's order.
    pub hypervisors: Vec<HypervisorOption>,
}

impl Options {
    /// None yet: what a report holds until its options are worked out from it.
    pub(crate) fn empty() -> Options {
        Options { wsl: Vec::new(), recommended_engine: Engine::Native, engine: Engine::Native, engines: Vec::new(), hypervisor: None, hypervisors: Vec::new() }
    }
}

/// A container engine, and what makes it the one Docker uses here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EngineOption {
    pub engine: Engine,
    pub actions: Vec<Action>,
}

/// A local hypervisor: what gets it here (`get`: nothing once it is installed and can run), then
/// what Vagrant needs to drive it (`vagrant`: Vagrant, its plugin, VMware's utility, in order).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HypervisorOption {
    pub provider: Provider,
    pub get: Vec<Action>,
    pub vagrant: Vec<Action>,
}

/// The engines the setup offers on `os`, in the order it shows them.
pub fn engines_on(os: &str) -> &'static [Engine] {
    match os {
        "macos" => &[Engine::DockerDesktop, Engine::Orbstack, Engine::Colima],
        "windows" => &[Engine::DockerDesktop],
        _ => &[Engine::DockerDesktop, Engine::Native, Engine::Colima],
    }
}

/// The engine the setup installs in one click on `os`: Docker Desktop on macOS and Windows,
/// Docker Engine on Linux.
pub fn recommended_engine(os: &str) -> Engine {
    if matches!(os, "macos" | "windows") { Engine::DockerDesktop } else { Engine::Native }
}

/// Docker answers: its CLI is installed and its daemon runs for this account.
pub fn docker_ready(r: &SystemReport) -> bool {
    r.docker.installed && r.docker_running
}

/// Installing `dependency` here: its install, or on a Linux distribution without apt (the guided
/// setup installs through it), the player's to do.
fn install(r: &SystemReport, dependency: Dependency) -> Action {
    if r.os == "linux" && !r.pkg_manager.installed {
        return Action::Manual { reason: ManualReason::Distribution, dependency: Some(dependency), url: own_install_url(dependency).map(str::to_owned) };
    }
    Action::Install { dependency }
}

/// Where the player installs `dependency` themselves, when the setup can't.
fn own_install_url(dependency: Dependency) -> Option<&'static str> {
    Some(match dependency {
        Dependency::Docker => "https://docs.docker.com/engine/install/",
        Dependency::Vagrant => "https://developer.hashicorp.com/vagrant/install",
        Dependency::Virtualbox => "https://www.virtualbox.org/wiki/Linux_Downloads",
        Dependency::Qemu => "https://www.qemu.org/download/",
        Dependency::Libvirt => "https://libvirt.org/",
        Dependency::VmwareUtility => "https://developer.hashicorp.com/vagrant/install/vmware",
        _ => return None,
    })
}

/// What turns WSL 2 on: nothing off Windows, once it is ready, or once a Docker engine runs
/// (proof enough, whatever the check could tell: #224); the restart once it waits for one; else
/// its install (also when the check couldn't tell).
fn wsl(r: &SystemReport) -> Vec<Action> {
    if r.os != "windows" || docker_ready(r) {
        return Vec::new();
    }
    match r.wsl.as_ref().map_or(WslState::Unknown, |w| w.state) {
        WslState::Ready => Vec::new(),
        WslState::NeedsReboot => vec![Action::Restart { feature: WindowsFeature::Wsl }],
        WslState::Missing | WslState::Unknown => vec![install(r, Dependency::Wsl)],
    }
}

/// What makes `engine` the one Docker uses: nothing when it is; switching to it when it runs; a
/// new login when the daemon refuses this account (Linux's docker group); starting it when Docker
/// is installed but doesn't answer; installing it when it is the recommended one and there is no
/// Docker; else its vendor's site.
fn engine_actions(r: &SystemReport, engine: Engine) -> Vec<Action> {
    let action = if docker_ready(r) && r.docker_engine == Some(engine) {
        return Vec::new();
    } else if r.docker_engines_running.contains(&engine) {
        Action::UseEngine { engine }
    } else if r.docker_denied && engine == Engine::Native {
        Action::Relogin
    } else if r.docker.installed && !docker_ready(r) {
        Action::StartEngine { engine }
    } else if engine == recommended_engine(r.os) && !r.docker.installed {
        install(r, Dependency::Docker)
    } else {
        Action::Manual { reason: ManualReason::Vendor, dependency: None, url: Some(engine.url().to_owned()) }
    };
    vec![action]
}

/// The hypervisor the setup installs as `provider` on `os`, when it installs it.
fn installable(provider: Provider, os: &str) -> Option<Dependency> {
    match provider {
        Provider::Virtualbox => Some(Dependency::Virtualbox),
        Provider::Qemu => Some(Dependency::Qemu),
        Provider::Utm if os == "macos" => Some(Dependency::Utm),
        Provider::Libvirt if os == "linux" => Some(Dependency::Libvirt),
        Provider::Hyperv if os == "windows" => Some(Dependency::Hyperv),
        _ => None,
    }
}

/// Where a hypervisor the setup doesn't install comes from (behind a login, or a licence).
fn vendor_url(provider: Provider) -> Option<&'static str> {
    Some(match provider {
        Provider::VmwareDesktop => "https://www.vmware.com/products/desktop-hypervisor/workstation-and-fusion",
        Provider::Parallels => "https://www.parallels.com/products/desktop/",
        Provider::Hyperv => "https://learn.microsoft.com/virtualization/hyper-v-on-windows/quick-start/enable-hyper-v",
        _ => return None,
    })
}

/// An installed hypervisor that still can't run labs once everything Vagrant needs is in (no
/// `/dev/kvm`, not in the kvm group, a stuck service): its `reason` says why.
pub fn cant_run(p: &ProviderStatus) -> bool {
    p.hypervisor == Some(true) && !p.available && p.reason.is_some() && p.needs.is_empty()
}

/// What gets hypervisor `p` here: nothing once it is installed (a new login when the player joined
/// libvirt's groups since logging in; else why it still can't run); the restart Hyper-V waits for
/// once the setup turned it on (`hyperv_restart`); its install; else its vendor's site.
fn hypervisor_get(r: &SystemReport, p: &ProviderStatus, hyperv_restart: bool) -> Vec<Action> {
    if p.hypervisor == Some(true) {
        let joined = r.groups_pending.iter().any(|g| crate::runtime::providers::LIBVIRT_GROUPS.contains(&g.as_str()));
        if p.provider == Provider::Libvirt && joined {
            return vec![Action::Relogin];
        }
        return match &p.reason {
            Some(reason) if cant_run(p) => vec![Action::Blocked { reason: reason.clone() }],
            _ => Vec::new(),
        };
    }
    if p.provider == Provider::Hyperv && hyperv_restart {
        return vec![Action::Restart { feature: WindowsFeature::Hyperv }];
    }
    vec![match installable(p.provider, r.os) {
        Some(dependency) => install(r, dependency),
        None => Action::Manual { reason: ManualReason::Vendor, dependency: None, url: vendor_url(p.provider).map(str::to_owned) },
    }]
}

/// What Vagrant needs to drive `p` ([`ProviderStatus::needs`], the list readiness reads), each
/// with the item it is about: `vagrant`, `plugin:<name>`, `vmware-utility`.
fn vagrant_steps(r: &SystemReport, p: &ProviderStatus) -> Vec<(String, Action)> {
    p.needs
        .iter()
        .filter_map(|need| {
            Some(match need {
                Need::Vagrant => ("vagrant".to_owned(), install(r, Dependency::Vagrant)),
                Need::Plugin => {
                    let plugin = p.plugin.clone()?;
                    (format!("plugin:{plugin}"), Action::Plugin { plugin })
                }
                Need::VmwareUtility => ("vmware-utility".to_owned(), install(r, Dependency::VmwareUtility)),
            })
        })
        .collect()
}

/// The local hypervisors of the machine check.
fn local(r: &SystemReport) -> impl Iterator<Item = &ProviderStatus> {
    r.vm_providers.iter().filter(|p| !p.remote)
}

/// What each choice takes on the machine `r` reports; `hyperv_restart`: the setup turned Hyper-V
/// on in this boot, so it waits for a restart.
pub fn options(r: &SystemReport, hyperv_restart: bool) -> Options {
    let offered = engines_on(r.os);
    let recommended = recommended_engine(r.os);
    let engine = match r.docker_engine {
        Some(e) if offered.contains(&e) => e,
        Some(_) => offered[0],
        None => recommended,
    };
    let hypervisors: Vec<HypervisorOption> = local(r)
        .map(|p| HypervisorOption {
            provider: p.provider,
            get: hypervisor_get(r, p, hyperv_restart),
            vagrant: vagrant_steps(r, p).into_iter().map(|(_, action)| action).collect(),
        })
        .collect();
    let installed = |o: &&HypervisorOption| local(r).any(|p| p.provider == o.provider && p.hypervisor == Some(true));
    let hypervisor = hypervisors
        .iter()
        .find(|o| installed(o) && o.get.is_empty())
        .or_else(|| hypervisors.iter().find(installed))
        .or_else(|| hypervisors.iter().find(|o| matches!(o.get.first(), Some(Action::Install { .. }))))
        .or_else(|| hypervisors.first())
        .map(|o| o.provider);
    Options {
        wsl: wsl(r),
        recommended_engine: recommended,
        engine,
        engines: offered.iter().map(|&e| EngineOption { engine: e, actions: engine_actions(r, e) }).collect(),
        hypervisor,
        hypervisors,
    }
}

#[cfg(test)]
mod tests;
