//! The hypervisors, servers and cloud accounts labs run on. The local hypervisors are Vagrant
//! providers: only what is usable on this machine is detected, and the runner is given the
//! provider. The servers and cloud accounts share the enum; labs run there through the Isothrum
//! runner (Proxmox and the clouds' `cloud-docker`, `cloud-vm` and `cloud-services` through
//! Terraform, ESXi through Vagrant), and they are never detected here.

mod kvm;
mod provider;
mod status;
mod unavailable;
mod vagrant_needs;
mod vbox_driver;
mod versions;

use crate::platform::exec::{run_read, timed_out};
use crate::platform::host::{Arch, Os};
use crate::platform::remembered::remembered;
use crate::platform::tool_output::vagrant::plugins as parse_plugin_versions;

pub(crate) use kvm::kvm_problem;
#[cfg(test)]
pub(crate) use kvm::{assume_devices_ready, kvm_group};
use kvm::{holds_virtualization, new_login_group};
pub use kvm::{login_groups, uses_kvm};
pub use provider::Provider;
pub use status::{AccountLacks, ProviderStatus};
pub use unavailable::Unavailable;
#[cfg(test)]
use vagrant_needs::fake_utility;
pub use vagrant_needs::{Need, UtilityState, needs, vmware_utility};
#[cfg(test)]
use versions::version_at_least;
use versions::{hypervisor_too_old, plugin_too_old};

/// How long VirtualBox's service gets to answer `VBoxManage list runningvms` before it reads as
/// unresponsive (wedged), not just slow.
const VBOX_SERVICE_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

const UTM_APP: &str = "/Applications/UTM.app";

/// Whether the Hyper-V feature is on: its Virtual Machine Management service is installed with it.
fn hyperv_enabled() -> bool {
    let root = std::env::var_os("SystemRoot").map_or_else(|| std::path::PathBuf::from("C:\\Windows"), std::path::PathBuf::from);
    root.join("System32").join("vmms.exe").is_file()
}

fn qemu_binary() -> &'static str {
    Arch::HERE.qemu_system()
}

/// Whether `provider`'s VMs need `mkisofs` on `os`: QEMU's on Linux. vagrant-qemu gives a VM on a
/// lab network (every lab VM on QEMU, through its network's hub) its settings in a cloud-init
/// seed, an ISO Vagrant's `create_iso` builds, with `mkisofs` on Linux (`hdiutil` on macOS,
/// which every Mac has). Without it, `vagrant up` fails at the lab's first VM.
pub(crate) fn needs_mkisofs(provider: Provider, os: Os) -> bool {
    provider == Provider::Qemu && os == Os::Linux
}

async fn tool_present(program: &'static str, args: &[&str]) -> bool {
    match run_read(program, args, None).await {
        // vmrun without arguments prints usage and exits non-zero, but it exists.
        Ok(_) | Err(crate::error::Error::CommandFailed { .. }) => true,
        // A tool that ran out of the probe's time is there, and slow (a cold VBoxSVC): not
        // absent, which would offer to install it again.
        Err(e) if crate::platform::exec::timed_out(&e) => {
            log::warn!("`{program}` took longer than the probe waits: read as installed");
            true
        }
        Err(_) => false,
    }
}

/// The Vagrant plugins installed, each with its version when it says one; why they can't be
/// listed when `vagrant plugin list` fails (not "none": a plugin that is there would read as
/// missing).
async fn installed_plugins() -> Result<Vec<(String, Option<String>)>, String> {
    remembered("vagrant plugin list", run_read("vagrant", &["plugin", "list"], None)).await.map(|o| parse_plugin_versions(&o)).map_err(|e| e.to_string())
}

/// What `detect` knows of the Vagrant plugins: the ones listed, or why they couldn't be.
type Plugins = Result<Vec<(String, Option<String>)>, String>;

/// The local hypervisors ready for a lab's VMs on this machine: Vagrant, the hypervisor and its
/// plugin (what the Machine page reports as ready), in [`Provider::ALL`]'s order.
pub async fn ready_here() -> Vec<Provider> {
    if run_read("vagrant", &["--version"], None).await.is_err() {
        return Vec::new();
    }
    ready(&detect(true).await, true)
}

/// Of `statuses` ([`detect`]'s), the local hypervisors ready for a lab's VMs: none without
/// Vagrant (`vagrant_installed`). The machine report carries them, so the webview never works
/// this out again.
pub fn ready(statuses: &[ProviderStatus], vagrant_installed: bool) -> Vec<Provider> {
    if !vagrant_installed {
        return Vec::new();
    }
    // `available` holds only where the hypervisor is there: one that isn't has a reason.
    statuses.iter().filter(|p| !p.remote && p.available).map(|p| p.provider).collect()
}

/// The hypervisor VM labs run on, of the `ready` ones ([`ready`]): `preferred` (the player's
/// Settings choice) when it is ready, else the first ready one. The one rule the VM self-test and
/// a lab's default place use.
pub fn pick(preferred: Option<Provider>, ready: &[Provider]) -> Option<Provider> {
    preferred.filter(|p| ready.contains(p)).or_else(|| ready.first().copied())
}

/// What the machine check finds for each provider this machine can run (its OS and CPU), the
/// Vagrant plugins read only when Vagrant is installed (`vagrant_installed`).
pub async fn detect(vagrant_installed: bool) -> Vec<ProviderStatus> {
    let plugins = if vagrant_installed { installed_plugins().await } else { Ok(Vec::new()) };
    let mut statuses = Vec::new();
    for provider in Provider::ALL {
        statuses.extend(detect_one(provider, vagrant_installed, &plugins).await);
    }
    statuses
}

/// `provider`'s status as the machine report gives it ([`detect`]), checked alone: none when it
/// isn't offered on this machine. What the server self-test reads before it runs a lab on ESXi.
pub async fn status(provider: Provider) -> Option<ProviderStatus> {
    let vagrant_installed = run_read("vagrant", &["--version"], None).await.is_ok();
    let plugins = if vagrant_installed { installed_plugins().await } else { Ok(Vec::new()) };
    detect_one(provider, vagrant_installed, &plugins).await
}

/// One provider's status, given whether Vagrant is installed and its `plugins`: none when it
/// isn't offered on this machine.
async fn detect_one(provider: Provider, vagrant_installed: bool, plugins: &Plugins) -> Option<ProviderStatus> {
    // Only surface providers that can run on this machine, not the whole catalogue.
    if !provider.applicable() {
        return None;
    }

    // The hypervisor layer (the VM software itself). QEMU on Linux without `mkisofs` reads as not
    // installed, with its own reason: its install brings it.
    let found = provider.hypervisor_installed().await;
    if !provider.offered(found == Some(true)) {
        return None;
    }
    let mkisofs_missing = found == Some(true) && needs_mkisofs(provider, Os::HERE) && !tool_present("mkisofs", &["-version"]).await;
    let hypervisor = if mkisofs_missing { Some(false) } else { found };

    let (plugin, plugin_version, plugin_installed) = plugin_layer(provider, vagrant_installed, plugins);
    // The hypervisor's own version, where its plugin is particular about it (UTM).
    let hypervisor_version = if hypervisor == Some(true) { provider.hypervisor_version().await } else { None };

    let unresponsive = provider == Provider::Virtualbox && hypervisor == Some(true) && virtualbox_stuck().await;

    // Vagrant's Hyper-V provider refuses every command (status reads too) from an account
    // that may not manage Hyper-V: said here, with the way out, not as a raw error mid-boot.
    let needs_admin = provider == Provider::Hyperv && hypervisor == Some(true) && !crate::platform::exec::may_manage_hyperv();

    // libvirt's socket and /dev/kvm are their groups': added to them since logging in (the
    // libvirt and QEMU installs do it), the player can't use them before logging in again.
    let joined = login_groups(provider, Os::HERE);
    let relogin = !joined.is_empty() && hypervisor == Some(true) && crate::platform::exec::groups_pending().await.iter().any(|g| joined.contains(g));

    // VMware's plugin drives VMware through HashiCorp's utility service, installed on its own.
    let utility = if provider.needs_vmware_utility() { Some(vmware_utility().await) } else { None };

    let detection = Detection {
        unresponsive,
        hypervisor,
        mkisofs_missing,
        hypervisor_version: hypervisor_version.as_deref(),
        plugin: plugin.as_deref(),
        plugin_installed,
        plugin_version: plugin_version.as_deref(),
        plugins_unknown: plugins.as_ref().err().map(String::as_str),
        utility,
        account: if needs_admin {
            Some(AccountLacks::HypervAdmin)
        } else if relogin {
            Some(AccountLacks::NewLogin)
        } else {
            None
        },
    };
    Some(status_of(provider, vagrant_installed, detection))
}

/// `provider`'s status from what was found of it (`detection`), given whether Vagrant is
/// installed: what Vagrant still needs to drive it, and why it can't run labs yet, if it can't.
fn status_of(provider: Provider, vagrant_installed: bool, detection: Detection<'_>) -> ProviderStatus {
    let needs = needs(vagrant_installed, detection.plugin_installed, detection.utility);
    // No VirtualBox VM starts while KVM holds the CPU's virtualization (Linux), installed or not:
    // said first, as nothing the setup installs mends it. VBoxManage answers without VirtualBox's
    // kernel driver, which every VM needs (Linux).
    let reason = (provider == Provider::Virtualbox)
        .then(holds_virtualization)
        .flatten()
        .or_else(|| unavailable_reason(provider, &needs, detection))
        .or_else(|| (provider == Provider::Virtualbox && detection.hypervisor == Some(true)).then(vbox_driver::problem).flatten());
    ProviderStatus {
        provider,
        remote: provider.is_remote(),
        available: reason.is_none(),
        hypervisor: detection.hypervisor,
        plugin: detection.plugin.map(str::to_owned),
        plugin_installed: detection.plugin_installed,
        utility: detection.utility,
        needs,
        reason,
        account_lacks: detection.account,
    }
}

/// The Vagrant layer of `provider` (the plugin that drives it): the plugin it needs, the version
/// installed, and whether that is installed at a version the launcher drives labs with. A
/// built-in provider (virtualbox, hyperv) has no plugin and reads installed; one whose plugins
/// couldn't be listed reads not installed, and `detect` says why.
fn plugin_layer(provider: Provider, vagrant_installed: bool, plugins: &Plugins) -> (Option<String>, Option<String>, bool) {
    let plugin = provider.plugin().map(str::to_string);
    let listed = plugins.as_deref().unwrap_or_default();
    let found = plugin.as_deref().and_then(|p| listed.iter().find(|(name, _)| name == p));
    let plugin_version = found.and_then(|(_, version)| version.clone());
    let plugin_installed = match &plugin {
        None => true,
        Some(_) => vagrant_installed && found.is_some() && plugin_too_old(provider, plugin_version.as_deref()).is_none(),
    };
    (plugin, plugin_version, plugin_installed)
}

/// Whether VirtualBox's service (`VBoxSVC`) is wedged. Installed is not the same as working:
/// `VBoxManage --version` never talks to the service, which can wedge so that every real call
/// hangs (a deploy then sits on "Bringing machine up" forever). Asks it something that needs the
/// service, briefly: stuck only when the app's own timeout cut the answer short.
async fn virtualbox_stuck() -> bool {
    crate::platform::exec::run_env_timed("VBoxManage", &["list", "runningvms"], None, &[], VBOX_SERVICE_WAIT).await.is_err_and(|e| timed_out(&e))
}

/// What `detect` found out about one provider: the per-provider facts `unavailable_reason` reads
/// to name the first layer that is missing (the hypervisor, its plugin, an admin right). Grouped
/// so the check reads one value rather than a handful of flags that always travel together;
/// what is still to install (Vagrant, global, among it) is the check's other argument.
#[derive(Clone, Copy)]
struct Detection<'a> {
    /// Whether the hypervisor's background service is wedged (a probe call timed out).
    unresponsive: bool,
    /// Whether the hypervisor is installed; `None` when there is nothing local to probe.
    hypervisor: Option<bool>,
    /// QEMU is there but not the `mkisofs` its VMs need ([`needs_mkisofs`]), so `hypervisor`
    /// reads false.
    mkisofs_missing: bool,
    /// The hypervisor's version, when it is installed and held to one (UTM).
    hypervisor_version: Option<&'a str>,
    /// The Vagrant plugin this provider needs, if it is not built in.
    plugin: Option<&'a str>,
    /// Whether that plugin is installed, at a version the launcher drives.
    plugin_installed: bool,
    /// Why the plugins couldn't be listed, when `vagrant plugin list` failed: `plugin_installed`
    /// then says nothing.
    plugins_unknown: Option<&'a str>,
    /// That plugin's installed version, when it is present and says one.
    plugin_version: Option<&'a str>,
    /// Where the Vagrant VMware Utility stands; `None` when the provider doesn't use it.
    utility: Option<UtilityState>,
    /// What this account lacks to use it, if anything.
    account: Option<AccountLacks>,
}

/// Why a provider can't be used here (None: it can), from what is still to install for it
/// (`needs`, the [`needs`] its status carries too, computed once) and what `detect` found out
/// about the provider: Vagrant missing first, then what is wrong with the
/// hypervisor itself (nothing setup installs mends that), then the first of its [`needs`]. So it
/// is `None` only when nothing is needed: readiness and the setup steps can't disagree.
fn unavailable_reason(provider: Provider, needs: &[Need], detection: Detection<'_>) -> Option<Unavailable> {
    let Detection { unresponsive, hypervisor, mkisofs_missing, hypervisor_version, plugin, plugin_version, plugins_unknown, utility, account, .. } = detection;
    let plugin = || plugin.unwrap_or("?").to_owned();
    if needs.contains(&Need::Vagrant) {
        Some(Unavailable::VagrantMissing)
    } else if unresponsive {
        Some(Unavailable::VirtualboxStuck)
    } else if hypervisor == Some(false) && provider == Provider::Hyperv {
        Some(Unavailable::HypervOff)
    } else if account == Some(AccountLacks::HypervAdmin) {
        Some(Unavailable::HypervNeedsAdmin)
    } else if mkisofs_missing {
        Some(Unavailable::MkisofsMissing)
    } else if hypervisor == Some(false) {
        Some(Unavailable::NotInstalled { program: provider.probe().map(|(program, _)| program.to_owned()) })
    } else if account == Some(AccountLacks::NewLogin) {
        Some(Unavailable::NewLogin { group: new_login_group(provider) })
    } else if let Some(why) = uses_kvm(provider, Os::HERE).then(kvm_problem).flatten() {
        Some(why)
    } else if let Some((name, version, min)) = hypervisor_too_old(provider, hypervisor_version) {
        Some(Unavailable::HypervisorTooOld { name: name.into(), version: version.into(), min: min.into() })
    } else if let Some((version, min)) = plugin_too_old(provider, plugin_version) {
        Some(Unavailable::PluginTooOld { plugin: plugin(), version: version.into(), min: min.into() })
    } else if let (Some(_), Some(why)) = (detection.plugin, plugins_unknown) {
        Some(Unavailable::PluginsUnknown { plugin: plugin(), reason: why.to_owned() })
    } else {
        needs.first().map(|need| match need {
            Need::Vagrant => Unavailable::VagrantMissing,
            Need::Plugin => Unavailable::PluginMissing { plugin: plugin() },
            Need::VmwareUtility if utility == Some(UtilityState::Stopped) => Unavailable::VmwareUtilityStopped,
            Need::VmwareUtility => Unavailable::VmwareUtilityMissing,
        })
    }
}

#[cfg(test)]
mod tests;
