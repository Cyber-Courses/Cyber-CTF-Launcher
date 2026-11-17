//! Machines as the machine check reports them, built by hand: the shapes the setup's plan is
//! tested on here, and which the webview's tests render the setup window on (each written, with
//! the setup options worked out for it, into `src/lib/tauri/generated/SetupShapes.ts`). Nothing
//! here reads this machine.

use crate::machine::system::docker::{Engine, EngineStatus};
use crate::machine::system::wsl::{WslReport, WslState};
use crate::machine::system::{CloudClis, PackageManager, SystemReport, Tool};
use isothrum_machine::providers::{AccountLacks, needs};
use isothrum_machine::{Arch, Host, HostOs};

use crate::runtime::providers::{Provider, ProviderStatus, Unavailable, UtilityState};

fn missing() -> Tool {
    Tool { installed: false, version: None }
}

fn found(version: &str) -> Tool {
    Tool { installed: true, version: Some(version.to_owned()) }
}

/// The hypervisors the check offers on `host` (none installed), then ESXi (remote), in
/// `Provider::ALL`'s order: the hypervisor table's ([`Provider::offered_on`]). The check detects
/// Vagrant's providers only.
fn offered(host: &Host) -> Vec<Provider> {
    Provider::ALL.into_iter().filter(|p| p.vagrant().is_some() && p.offered_on(host, false)).collect()
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
    // Built as the check sends it: isothrum-machine's status is its own to make.
    serde_json::from_value(serde_json::json!({
        "provider": provider,
        "remote": remote,
        "available": reason.is_none(),
        "hypervisor": hypervisor,
        "plugin": plugin,
        "pluginInstalled": plugin_installed,
        "utility": utility,
        "needs": needs,
        "reason": reason,
        "accountLacks": null,
    }))
    .expect("a provider status")
}

/// A machine with nothing but its OS and its package manager.
pub(crate) fn bare(os: HostOs, arch: Arch) -> SystemReport {
    let version = match os {
        HostOs::Windows => "v1.9.25200",
        HostOs::Linux => "apt 2.9.8 (amd64)",
        HostOs::Macos => "Homebrew 4.6.3",
    };
    let pkg_manager = PackageManager { name: isothrum_machine::host::usual_packages(os).into(), installed: true, version: Some(version.into()) };
    let wsl = (os == HostOs::Windows).then_some(WslReport { state: WslState::Missing, reopens_after_restart: true, virtualization_off: false });
    let mut r = SystemReport {
        os,
        arch: crate::platform::cpu_name(arch),
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
        vm_providers: offered(&crate::runtime::providers::testing::host_on(os, arch, None)).into_iter().map(|p| status(p, false, false, false, None)).collect(),
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
    r.vm_providers = r.vm_providers.iter().map(|p| status(p.provider.into(), vagrant, p.hypervisor == Some(true), p.plugin_installed, p.utility)).collect();
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
    bare(HostOs::Windows, Arch::Amd64)
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
    bare(HostOs::Linux, Arch::Amd64)
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
    awaits_relogin(Provider::Libvirt, false, &["libvirt", "kvm"], "libvirt")
}

/// Debian once the setup installed QEMU and Vagrant: the QEMU install added the player to the kvm
/// group (`/dev/kvm` is `root:kvm 0660` there), which applies from the next login.
pub(crate) fn debian_qemu_awaits_relogin() -> SystemReport {
    let kvm = crate::runtime::providers::kvm_group();
    awaits_relogin(Provider::Qemu, true, &[kvm.as_str()], &kvm)
}

/// Debian with Vagrant, vagrant-qemu and QEMU's `qemu-system` but not `mkisofs`, which QEMU's VMs
/// need on Linux (QEMU installed before the setup brought genisoimage with it): QEMU reads as not
/// installed, saying why.
pub(crate) fn debian_qemu_without_mkisofs() -> SystemReport {
    part_missing(Provider::Qemu, &Unavailable::MkisofsMissing)
}

/// Debian with Vagrant, vagrant-libvirt and libvirt but no virtiofsd, which libvirt shares the
/// attack box's folder with a VM through (libvirt installed before the setup brought virtiofsd
/// with it): libvirt reads as not installed, saying why.
pub(crate) fn debian_libvirt_without_virtiofsd() -> SystemReport {
    part_missing(Provider::Libvirt, &Unavailable::VirtiofsdMissing { hypervisor: "libvirt".into() })
}

/// Debian with Vagrant, vagrant-qemu and QEMU (with `mkisofs`) but no virtiofsd, which
/// vagrant-qemu starts to share the attack box's folder with a VM (QEMU installed before the setup
/// brought virtiofsd with it): QEMU reads as not installed, saying why.
pub(crate) fn debian_qemu_without_virtiofsd() -> SystemReport {
    part_missing(Provider::Qemu, &Unavailable::VirtiofsdMissing { hypervisor: "QEMU".into() })
}

/// Debian with Vagrant, `provider` and its plugin, but not a program its VMs need beside it
/// (`reason`), which its install brings: `provider` reads as not installed, saying why.
fn part_missing(provider: Provider, reason: &Unavailable) -> SystemReport {
    let mut r = with(bare_debian(), false, |r| hypervisor(r, provider, true));
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == provider) {
        (p.hypervisor, p.available, p.reason) = (Some(false), false, Some(reason.clone()));
    }
    r.ready_hypervisors = crate::runtime::providers::ready(&r.vm_providers, true);
    r.setup = super::options(&r, false);
    r
}

/// Debian with `provider` installed (its plugin as `plugin`) and the player in `groups` since
/// logging in: `provider` waits for the new login that applies `group`.
fn awaits_relogin(provider: Provider, plugin: bool, groups: &[&str], group: &str) -> SystemReport {
    let mut r = with(bare_debian(), false, |r| {
        hypervisor(r, provider, plugin);
        r.groups_pending = groups.iter().map(|g| (*g).to_owned()).collect();
    });
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == provider) {
        p.available = false;
        p.reason = Some(Unavailable::NewLogin { group: group.to_owned() });
        p.account_lacks = Some(AccountLacks::NewLogin);
    }
    r.ready_hypervisors = crate::runtime::providers::ready(&r.vm_providers, true);
    r.setup = super::options(&r, false);
    r
}

/// Fedora: Linux with dnf, which the guided setup installs through as it does through apt.
pub(crate) fn bare_fedora() -> SystemReport {
    with(bare_debian(), false, |r| r.pkg_manager = PackageManager { name: "dnf".into(), installed: true, version: Some("4.24.0".into()) })
}

/// Arch: Linux with neither apt nor dnf, which the guided setup doesn't install through yet.
pub(crate) fn bare_arch() -> SystemReport {
    with(bare_debian(), false, |r| r.pkg_manager = PackageManager { name: "apt".into(), installed: false, version: None })
}

/// An Apple Silicon Mac with Docker Desktop running, but no Vagrant and no hypervisor.
pub(crate) fn mac_docker_without_vagrant() -> SystemReport {
    with(bare(HostOs::Macos, Arch::Arm64), false, |r| docker_running(r, Engine::DockerDesktop))
}

/// An Apple Silicon Mac with OrbStack in use and Docker Desktop running beside it.
pub(crate) fn mac_two_engines() -> SystemReport {
    with(bare(HostOs::Macos, Arch::Arm64), false, |r| {
        docker_running(r, Engine::Orbstack);
        r.docker_engines_running = vec![Engine::Orbstack, Engine::DockerDesktop];
        r.docker_engines.push(EngineStatus { engine: Engine::DockerDesktop, context: "desktop-linux".into(), running: true, current: false });
    })
}

/// A Mac with VMware Fusion and Vagrant, but neither the VMware plugin nor HashiCorp's utility.
pub(crate) fn mac_vmware_without_its_parts() -> SystemReport {
    with(bare(HostOs::Macos, Arch::Arm64), false, |r| {
        docker_running(r, Engine::DockerDesktop);
        hypervisor(r, Provider::VmwareDesktop, false);
    })
}

/// An Apple Silicon Mac set up: Docker Desktop running, UTM with Vagrant and its plugin.
pub(crate) fn mac_ready() -> SystemReport {
    with(bare(HostOs::Macos, Arch::Arm64), false, |r| {
        docker_running(r, Engine::DockerDesktop);
        hypervisor(r, Provider::Utm, true);
    })
}

/// Debian once the setup installed VirtualBox and Vagrant on a machine running a kernel older than
/// its package sources' (not restarted since an update): the running kernel's headers were gone,
/// so the install brought the newer kernel with its headers, and VirtualBox's driver waits for the
/// restart into it.
pub(crate) fn debian_virtualbox_awaits_restart() -> SystemReport {
    let mut r = with(bare_debian(), false, |r| hypervisor(r, Provider::Virtualbox, true));
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == Provider::Virtualbox) {
        p.available = false;
        p.reason = Some(Unavailable::KernelRestart { kernel: "6.12.116+deb13-amd64".into() });
    }
    r.ready_hypervisors = crate::runtime::providers::ready(&r.vm_providers, true);
    r.setup = super::options(&r, false);
    r
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
        ("debianQemuAwaitsRelogin", debian_qemu_awaits_relogin()),
        ("debianQemuWithoutMkisofs", debian_qemu_without_mkisofs()),
        ("debianLibvirtWithoutVirtiofsd", debian_libvirt_without_virtiofsd()),
        ("debianQemuWithoutVirtiofsd", debian_qemu_without_virtiofsd()),
        ("bareFedora", bare_fedora()),
        ("bareArch", bare_arch()),
        ("macDockerWithoutVagrant", mac_docker_without_vagrant()),
        ("macTwoEngines", mac_two_engines()),
        ("macVmwareWithoutItsParts", mac_vmware_without_its_parts()),
        ("macReady", mac_ready()),
    ]
}
