//! The system check: which tools, engines, hypervisors and package manager this machine has,
//! and what Isoloom says it can run. Also the machine's live metrics and its setup window.

pub mod docker;
pub mod metrics;
pub mod wsl;

use serde::Serialize;

pub use isothrum_machine::tools::{CloudClis, PackageManager, Tool};

use crate::platform::exec::docker_denied_error;
use crate::runtime::providers::{self, ProviderStatus};
use isothrum_machine::HostOs;

/// What the machine check found on this computer: the OS, the tools, the container engines and
/// hypervisors, and what the guided setup takes from here.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    /// The OS.
    #[ts(as = "OsId")]
    pub os: HostOs,
    /// The CPU architecture, by Rust's name for it (`x86_64`, `aarch64`).
    #[ts(as = "CpuName")]
    pub arch: &'static str,
    /// The OS's package manager.
    pub pkg_manager: PackageManager,
    /// The Docker CLI.
    pub docker: Tool,
    /// The Docker daemon answers (Docker Desktop / engine is started).
    pub docker_running: bool,
    /// The daemon runs but refused this account (not in the `docker` group yet): starting the
    /// engine won't help; `docker_denied_hint` says what will (as an error with its code, for
    /// the webview to say in the player's language).
    pub docker_denied: bool,
    /// Why the daemon refused this account and what to do, as a coded error, when it did.
    pub docker_denied_hint: Option<crate::error::Wire>,
    /// Which Docker-compatible engine answers, when one does.
    pub docker_engine: Option<docker::Engine>,
    /// Every engine whose Docker context answers right now. Several can run side by side
    /// (e.g. Docker Desktop and OrbStack); `docker_engine` is the one the CLI uses.
    pub docker_engines_running: Vec<docker::Engine>,
    /// Every Docker-compatible engine found on this machine (one per CLI context that names
    /// one or answers), each with its own state: the Machine page lists them all, not only
    /// the one the CLI uses.
    pub docker_engines: Vec<docker::EngineStatus>,
    /// Docker Compose (`docker compose`).
    pub docker_compose: Tool,
    /// Vagrant.
    pub vagrant: Tool,
    /// A local terraform binary (preferred for provisioning state; container is the fallback).
    pub terraform: Tool,
    /// VMware's OVF Tool, which the ESXi Vagrant plugin uses to upload VMs (ships with
    /// VMware Fusion / Workstation, or standalone from Broadcom).
    pub ovftool: Tool,
    /// Where the OVF Tool was found (Fusion's or Workstation's copy, the app's, or `PATH`), when
    /// one was, even if it doesn't run.
    pub ovftool_location: Option<crate::machine::tools::ovftool::Location>,
    /// The cloud providers' CLIs.
    pub cloud_clis: CloudClis,
    /// Vagrant providers (local hypervisors and remote hosts such as ESXi).
    /// Whether a given VM lab can run also depends on its boxes existing for
    /// this architecture: x86-only boxes (e.g. pfSense) cannot run on ARM hosts
    /// with a local hypervisor, but can on a remote x86 host.
    pub vm_providers: Vec<ProviderStatus>,
    /// The local hypervisors of `vm_providers` ready for a lab's VMs ([`providers::ready`]).
    pub ready_hypervisors: Vec<providers::Provider>,
    /// Windows: whether WSL 2 is on, waits for a restart, or is missing (`None` elsewhere).
    pub wsl: Option<wsl::WslReport>,
    /// The groups this account joined since it logged in (Linux: the docker, libvirt and kvm
    /// groups the installs add it to), which apply from its next login.
    pub groups_pending: Vec<String>,
    /// What each choice of the guided setup takes on this machine ([`crate::machine::setup`]):
    /// the setup window renders these, never working them out itself.
    pub setup: crate::machine::setup::Options,
}

/// Checks this machine through isothrum-machine ([`isothrum_machine::check::check`]): the tools,
/// the container engines, the hypervisors, WSL and the groups pending a login; then the OVF Tool
/// and what the guided setup takes from there.
pub async fn system_check(dirs: crate::platform::AppDirs) -> SystemReport {
    let data = dirs.data().ok().map(std::path::Path::to_path_buf);
    // A tool installed since the last check (from the app or not) is found without a restart.
    let cx = providers::machine_setup().await;
    crate::platform::env_path::refresh(&cx).await;
    let (found, (ovftool, ovftool_location)) =
        tokio::join!(Box::pin(isothrum_machine::check::check(&cx, wsl::marks(&dirs))), crate::machine::tools::ovftool::probe(&cx, data.as_deref()));
    let docker_denied = found.daemon.denied();
    let host = &cx.host;
    let mut report = SystemReport {
        os: host.os,
        arch: crate::platform::cpu_name(host.arch),
        pkg_manager: found.packages,
        docker: found.docker,
        docker_running: found.daemon.running(),
        docker_denied,
        docker_denied_hint: docker_denied.then(|| docker_denied_error().to_wire()),
        docker_engine: found.daemon.engine,
        docker_engines_running: isothrum_machine::docker::running_of(&found.engines),
        docker_engines: found.engines,
        docker_compose: found.compose,
        vagrant: found.vagrant,
        terraform: found.terraform,
        ovftool,
        ovftool_location,
        cloud_clis: found.clouds,
        ready_hypervisors: found.ready.into_iter().map(providers::Provider::from).collect(),
        vm_providers: found.providers,
        wsl: found.wsl.map(|w| wsl::report(&dirs, &w)),
        groups_pending: found.groups_pending,
        setup: crate::machine::setup::Options::empty(),
    };
    // Hyper-V turned on from here starts at the next restart: until then the setup asks for it.
    let hyperv_restart = host.os == HostOs::Windows && wsl::hyperv_on_this_boot(&dirs);
    report.setup = crate::machine::setup::options(&report, hyperv_restart);
    report
}

/// An OS the launcher runs on, by its id (`macos`), as isothrum-machine writes it. (The webview's
/// `Os`: a type only, for the report's TypeScript.)
#[derive(ts_rs::TS)]
#[ts(rename = "Os", rename_all = "lowercase")]
#[expect(dead_code, reason = "a type only: never built, its variants are the webview's names")]
enum OsId {
    Macos,
    Windows,
    Linux,
}

/// A CPU the launcher runs on, by Rust's name for it (`std::env::consts::ARCH`). (The webview's
/// `Arch`: a type only, for the report's TypeScript.)
#[derive(ts_rs::TS)]
#[ts(rename = "Arch")]
#[expect(dead_code, reason = "a type only: never built, its variants are the webview's names")]
enum CpuName {
    #[ts(rename = "x86_64")]
    X86_64,
    #[ts(rename = "aarch64")]
    Aarch64,
}

#[cfg(test)]
mod tests;
