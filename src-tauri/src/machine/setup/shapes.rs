//! Machines as the machine check reports them, built by hand: the shapes the setup's plan is
//! tested on here, and which the webview's tests render the setup window on (each written, with
//! the setup options worked out for it, into `src/lib/tauri/generated/SetupShapes.ts`). Nothing
//! here reads this machine.

use crate::machine::system::docker::{Engine, EngineStatus};
use crate::machine::system::wsl::{WslReport, WslState};
use crate::machine::system::{CloudClis, PkgManager, SystemReport, Tool};
use crate::runtime::providers::{Provider, ProviderStatus, Unavailable, UtilityState, needs};

fn missing() -> Tool {
    Tool { installed: false, version: None }
}

fn found(version: &str) -> Tool {
    Tool { installed: true, version: Some(version.to_owned()) }
}

/// The local hypervisors the check offers on `os` with CPU `arch`, then the servers (remote), in
/// `Provider::ALL`'s order.
fn offered(os: &str, arch: &str) -> Vec<Provider> {
    let local: &[Provider] = match (os, arch) {
        ("macos", "aarch64") => &[Provider::VmwareDesktop, Provider::Parallels, Provider::Qemu, Provider::Utm],
        ("macos", _) => &[Provider::Virtualbox, Provider::VmwareDesktop, Provider::Parallels, Provider::Qemu],
        ("windows", _) => &[Provider::Virtualbox, Provider::VmwareDesktop, Provider::Hyperv, Provider::Qemu],
        _ => &[Provider::Virtualbox, Provider::VmwareDesktop, Provider::Libvirt, Provider::Qemu],
    };
    local.iter().copied().chain(Provider::SERVERS.iter().rev().copied()).collect()
}

/// `provider`'s status as the check reports it, from whether it (`installed`) and its plugin are
/// installed, with Vagrant (`vagrant`) or not; VMware's utility missing unless `utility`.
fn status(provider: Provider, vagrant: bool, installed: bool, plugin_installed: bool, utility: Option<UtilityState>) -> ProviderStatus {
    let plugin = provider.plugin().map(str::to_owned);
    let plugin_installed = plugin.is_none() || (vagrant && plugin_installed);
    let utility = if provider.needs_vmware_utility() { Some(utility.unwrap_or(UtilityState::Missing)) } else { None };
    let needs = needs(vagrant, plugin_installed, utility);
    let remote = provider.is_remote();
    let hypervisor = (!remote).then_some(installed);
    let reason = if !vagrant {
        Some(Unavailable::VagrantMissing)
    } else if hypervisor == Some(false) {
        Some(Unavailable::NotInstalled { program: None })
    } else if !plugin_installed {
        Some(Unavailable::PluginMissing { plugin: plugin.clone().unwrap_or_else(|| "?".into()) })
    } else if !needs.is_empty() {
        Some(Unavailable::VmwareUtilityMissing)
    } else {
        None
    };
    ProviderStatus { provider, remote, available: reason.is_none(), hypervisor, plugin, plugin_installed, utility, needs, reason, account_lacks: None }
}

/// A machine with nothing but its OS and its package manager.
pub(crate) fn bare(os: &'static str, arch: &'static str) -> SystemReport {
    let pkg_manager = match os {
        "windows" => PkgManager { name: "winget".into(), installed: true, version: Some("v1.9.25200".into()) },
        "linux" => PkgManager { name: "apt".into(), installed: true, version: Some("apt 2.9.8 (amd64)".into()) },
        _ => PkgManager { name: "Homebrew".into(), installed: true, version: Some("Homebrew 4.6.3".into()) },
    };
    let wsl = (os == "windows").then_some(WslReport { state: WslState::Missing, reopens_after_restart: true, virtualization_off: false });
    let mut r = SystemReport {
        os,
        arch,
        pkg_manager,
        docker: missing(),
        docker_running: false,
        docker_denied: false,
        docker_denied_hint: None,
        docker_engine: None,
        docker_engines_running: Vec::new(),
        docker_engines: Vec::new(),
        docker_compose: missing(),
        vagrant: missing(),
        terraform: missing(),
        ovftool: missing(),
        ovftool_location: None,
        cloud_clis: CloudClis { aws: missing(), azure: missing(), gcloud: missing() },
        vm_providers: offered(os, arch).into_iter().map(|p| status(p, false, false, false, None)).collect(),
        ready_hypervisors: Vec::new(),
        wsl,
        groups_pending: Vec::new(),
        setup: super::Options::empty(),
    };
    r.setup = super::options(&r, false);
    r
}

/// `r` with `edit` made to it, then its derived parts worked out again as the check does: each
/// hypervisor's needs and readiness, the ready ones, the setup options (`hyperv_restart`: Hyper-V
/// was turned on in this boot).
fn with(mut r: SystemReport, hyperv_restart: bool, edit: impl FnOnce(&mut SystemReport)) -> SystemReport {
    edit(&mut r);
    let vagrant = r.vagrant.installed;
    r.vm_providers = r.vm_providers.iter().map(|p| status(p.provider, vagrant, p.hypervisor == Some(true), p.plugin_installed, p.utility)).collect();
    r.ready_hypervisors = crate::runtime::providers::ready(&r.vm_providers, vagrant);
    r.setup = super::options(&r, hyperv_restart);
    r
}

/// Docker's CLI and Compose installed, and `engine` running and in use.
fn docker_running(r: &mut SystemReport, engine: Engine) {
    r.docker = found("Docker version 28.4.0, build d8eb465");
    r.docker_compose = found("2.39.2");
    r.docker_running = true;
    r.docker_engine = Some(engine);
    r.docker_engines_running = vec![engine];
    let context = if engine == Engine::Native { "default" } else { "desktop-linux" };
    r.docker_engines = vec![EngineStatus { engine, context: context.into(), running: true, current: true }];
}

/// `provider` installed (its plugin too when `plugin`), with Vagrant.
fn hypervisor(r: &mut SystemReport, provider: Provider, plugin: bool) {
    r.vagrant = found("Vagrant 2.4.9");
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == provider) {
        p.hypervisor = Some(true);
        p.plugin_installed = plugin;
    }
}

/// Windows 11, nothing installed: WSL 2 missing.
pub(crate) fn bare_windows() -> SystemReport {
    bare("windows", "x86_64")
}

/// Windows once "Turn on WSL 2" ran: it waits for a restart.
pub(crate) fn windows_wsl_waits_for_restart() -> SystemReport {
    with(bare_windows(), false, |r| r.wsl = Some(WslReport { state: WslState::NeedsReboot, reopens_after_restart: true, virtualization_off: false }))
}

/// Windows after the restart: WSL 2 ready, Docker Desktop installed but not started.
pub(crate) fn windows_docker_stopped() -> SystemReport {
    with(bare_windows(), false, |r| {
        r.wsl = Some(WslReport { state: WslState::Ready, reopens_after_restart: true, virtualization_off: false });
        r.docker = found("Docker version 28.4.0, build d8eb465");
    })
}

/// Windows once the setup turned Hyper-V on, before the restart that starts it.
pub(crate) fn windows_hyperv_waits_for_restart() -> SystemReport {
    with(bare_windows(), true, |_| {})
}

/// Debian 13, nothing installed.
pub(crate) fn bare_debian() -> SystemReport {
    bare("linux", "x86_64")
}

/// Debian once Docker Engine is installed: its daemon runs, but refuses this account until the
/// docker group applies at the next login.
pub(crate) fn debian_docker_denied() -> SystemReport {
    with(bare_debian(), false, |r| {
        r.docker = found("Docker version 28.4.0, build d8eb465");
        r.docker_compose = found("2.39.2");
        r.docker_denied = true;
        r.docker_engines = vec![EngineStatus { engine: Engine::Native, context: "default".into(), running: false, current: true }];
    })
}

/// Debian with libvirt and Vagrant installed, the vagrant-libvirt plugin not yet.
pub(crate) fn debian_libvirt_without_plugin() -> SystemReport {
    with(bare_debian(), false, |r| hypervisor(r, Provider::Libvirt, false))
}

/// Debian with libvirt installed, but KVM unusable: nothing the setup installs mends it.
pub(crate) fn debian_libvirt_without_kvm() -> SystemReport {
    let mut r = with(bare_debian(), false, |r| hypervisor(r, Provider::Libvirt, true));
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == Provider::Libvirt) {
        p.available = false;
        p.reason = Some(Unavailable::NoKvm);
    }
    r.ready_hypervisors = crate::runtime::providers::ready(&r.vm_providers, true);
    r.setup = super::options(&r, false);
    r
}

/// Debian once the setup installed libvirt and Vagrant: the player joined the libvirt and kvm
/// groups, which apply from the next login, so libvirt can't be used yet.
pub(crate) fn debian_libvirt_awaits_relogin() -> SystemReport {
    let mut r = with(bare_debian(), false, |r| {
        hypervisor(r, Provider::Libvirt, false);
        r.groups_pending = vec!["libvirt".into(), "kvm".into()];
    });
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == Provider::Libvirt) {
        p.available = false;
        p.reason = Some(Unavailable::NewLogin);
        p.account_lacks = Some(crate::runtime::providers::AccountLacks::NewLogin);
    }
    r.ready_hypervisors = crate::runtime::providers::ready(&r.vm_providers, true);
    r.setup = super::options(&r, false);
    r
}

/// Fedora: Linux without apt, which the guided setup doesn't install through yet.
pub(crate) fn bare_fedora() -> SystemReport {
    with(bare_debian(), false, |r| r.pkg_manager = PkgManager { name: "apt".into(), installed: false, version: None })
}

/// An Apple Silicon Mac with Docker Desktop running, but no Vagrant and no hypervisor.
pub(crate) fn mac_docker_without_vagrant() -> SystemReport {
    with(bare("macos", "aarch64"), false, |r| docker_running(r, Engine::DockerDesktop))
}

/// An Apple Silicon Mac with OrbStack in use and Docker Desktop running beside it.
pub(crate) fn mac_two_engines() -> SystemReport {
    with(bare("macos", "aarch64"), false, |r| {
        docker_running(r, Engine::Orbstack);
        r.docker_engines_running = vec![Engine::Orbstack, Engine::DockerDesktop];
        r.docker_engines.push(EngineStatus { engine: Engine::DockerDesktop, context: "desktop-linux".into(), running: true, current: false });
    })
}

/// A Mac with VMware Fusion and Vagrant, but neither the VMware plugin nor HashiCorp's utility.
pub(crate) fn mac_vmware_without_its_parts() -> SystemReport {
    with(bare("macos", "aarch64"), false, |r| {
        docker_running(r, Engine::DockerDesktop);
        hypervisor(r, Provider::VmwareDesktop, false);
    })
}

/// An Apple Silicon Mac set up: Docker Desktop running, UTM with Vagrant and its plugin.
pub(crate) fn mac_ready() -> SystemReport {
    with(bare("macos", "aarch64"), false, |r| {
        docker_running(r, Engine::DockerDesktop);
        hypervisor(r, Provider::Utm, true);
    })
}

/// Every shape, by the name the webview's tests know it by.
pub(crate) fn all() -> Vec<(&'static str, SystemReport)> {
    vec![
        ("bareWindows", bare_windows()),
        ("windowsWslWaitsForRestart", windows_wsl_waits_for_restart()),
        ("windowsDockerStopped", windows_docker_stopped()),
        ("windowsHypervWaitsForRestart", windows_hyperv_waits_for_restart()),
        ("bareDebian", bare_debian()),
        ("debianDockerDenied", debian_docker_denied()),
        ("debianLibvirtWithoutPlugin", debian_libvirt_without_plugin()),
        ("debianLibvirtWithoutKvm", debian_libvirt_without_kvm()),
        ("debianLibvirtAwaitsRelogin", debian_libvirt_awaits_relogin()),
        ("bareFedora", bare_fedora()),
        ("macDockerWithoutVagrant", mac_docker_without_vagrant()),
        ("macTwoEngines", mac_two_engines()),
        ("macVmwareWithoutItsParts", mac_vmware_without_its_parts()),
        ("macReady", mac_ready()),
    ]
}
