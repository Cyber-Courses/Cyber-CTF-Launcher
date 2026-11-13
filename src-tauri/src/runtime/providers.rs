//! The hypervisors, servers and cloud accounts labs run on. The local hypervisors are Vagrant
//! providers: only what is usable on this machine is detected, and the runner is given the
//! provider. The servers and cloud accounts share the enum; labs run there through the Isothrum
//! runner (Proxmox and the clouds' `cloud-docker`, `cloud-vm` and `cloud-services` through
//! Terraform, ESXi through Vagrant), and they are never detected here.

mod kvm;
mod unavailable;
mod vagrant_needs;
mod vbox_driver;
mod versions;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::platform::exec::{run_read, timed_out};
use crate::platform::remembered::remembered;

pub(crate) use kvm::kvm_problem;
#[cfg(test)]
pub(crate) use kvm::{assume_devices_ready, kvm_group};
use kvm::{holds_virtualization, new_login_group};
pub use kvm::{login_groups, uses_kvm};
pub use unavailable::Unavailable;
#[cfg(test)]
use vagrant_needs::fake_utility;
pub use vagrant_needs::{Need, UtilityState, needs, vmware_utility};
#[cfg(test)]
use versions::version_at_least;
use versions::{hypervisor_too_old, parse_plugin_versions, plugin_too_old};

/// How long VirtualBox's service gets to answer `VBoxManage list runningvms` before it reads as
/// unresponsive (wedged), not just slow.
const VBOX_SERVICE_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// Where a lab's machines can run: a hypervisor here or a server (each a Vagrant provider, its id
/// the value given to `vagrant up --provider`), Proxmox (the runner's Terraform), or a cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Oracle VirtualBox.
    Virtualbox,
    /// VMware Fusion (macOS) or Workstation (Windows, Linux).
    VmwareDesktop,
    /// Windows' Hyper-V.
    Hyperv,
    /// Parallels Desktop (macOS).
    Parallels,
    /// libvirt with KVM (Linux).
    Libvirt,
    /// QEMU on its own, without libvirt.
    Qemu,
    /// UTM (QEMU front end for macOS). Runs ARM guests natively on Apple
    /// Silicon and x86 guests under emulation (slow). Plugin `vagrant_utm`.
    Utm,
    /// Remote VMware ESXi host (plugin `vagrant-vmware-esxi`).
    VmwareEsxi,
    /// Remote Proxmox VE host (the runner's Terraform, bpg/proxmox).
    Proxmox,
    /// The player's AWS account (the runner's Terraform: a lab's `cloud-docker`, `cloud-vm` or `cloud-services`). Not a Vagrant
    /// provider and never detected locally, so it is not in `ALL`.
    Aws,
    /// The player's Azure subscription (the runner's Terraform: a lab's `cloud-docker`, `cloud-vm` or `cloud-services`). Like
    /// AWS: not a Vagrant provider, never detected locally, not in `ALL`.
    Azure,
    /// The player's Google Cloud project (the runner's Terraform: a lab's `cloud-docker`, `cloud-vm` or `cloud-services`). Like
    /// AWS and Azure: not a Vagrant provider, never detected locally, not in `ALL`.
    Gcp,
    /// The player's `DigitalOcean` account (the runner's Terraform: a lab's `cloud-docker`, `cloud-vm` or `cloud-services`).
    /// Authenticated with an API token; not a Vagrant provider, never detected, not in `ALL`.
    #[serde(rename = "digitalocean")]
    DigitalOcean,
    /// The player's Linode account (the runner's Terraform: a lab's `cloud-docker`, `cloud-vm` or `cloud-services`). Like
    /// `DigitalOcean`: API-token auth, not a Vagrant provider, never detected, not in `ALL`.
    Linode,
    /// The player's Oracle Cloud tenancy (the runner's Terraform: a lab's `cloud-docker`,
    /// `cloud-vm` or `cloud-services`). Authenticated with an API signing key in ~/.oci/config; not a Vagrant provider, never detected, not in `ALL`.
    Oci,
}

impl Provider {
    /// The player's own servers (as opposed to cloud accounts), in the order the launcher lists
    /// them. The webview reads them as `SERVER_PROVIDERS` (`testing/ts_bindings.rs`).
    pub const SERVERS: [Provider; 2] = [Provider::Proxmox, Provider::VmwareEsxi];

    /// The cloud accounts, in the order the launcher lists them.
    pub const CLOUDS: [Provider; 6] = [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci];

    /// The local hypervisors and the servers, not the clouds: the providers the machine check
    /// considers ([`detect`]).
    pub const ALL: [Provider; 9] = [
        Provider::Virtualbox,
        Provider::VmwareDesktop,
        Provider::Hyperv,
        Provider::Parallels,
        Provider::Libvirt,
        Provider::Qemu,
        Provider::Utm,
        Provider::VmwareEsxi,
        Provider::Proxmox,
    ];

    /// Its id: `vagrant up --provider`'s for a hypervisor, and the runner's and the webview's for
    /// every provider.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Virtualbox => "virtualbox",
            Provider::VmwareDesktop => "vmware_desktop",
            Provider::Hyperv => "hyperv",
            Provider::Parallels => "parallels",
            Provider::Libvirt => "libvirt",
            Provider::Qemu => "qemu",
            Provider::Utm => "utm",
            Provider::VmwareEsxi => "vmware_esxi",
            Provider::Proxmox => "proxmox",
            Provider::Aws => "aws",
            Provider::Azure => "azure",
            Provider::Gcp => "gcp",
            Provider::DigitalOcean => "digitalocean",
            Provider::Linode => "linode",
            Provider::Oci => "oci",
        }
    }

    /// The provider whose id (`vagrant up --provider`'s, the runner's) is `id`, if the launcher
    /// knows one.
    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.into_iter().chain(Provider::CLOUDS).find(|p| p.id() == id)
    }

    /// The provider's name as the player reads it on the OS `os` (`std::env::consts::OS`):
    /// VMware's desktop hypervisor is Fusion on macOS and Workstation elsewhere, so nobody is
    /// pointed at a product their OS doesn't have; every other provider is its [`Provider::label`].
    pub fn label_on(self, os: &str) -> &'static str {
        match (self, os) {
            (Provider::VmwareDesktop, "macos") => "VMware Fusion",
            (Provider::VmwareDesktop, "linux" | "windows") => "VMware Workstation",
            _ => self.label(),
        }
    }

    /// The provider's name as the player reads it ("VirtualBox", "QEMU"), whatever the OS. The
    /// webview's names come from here: the generator (`testing/ts_bindings.rs`) writes
    /// `PROVIDER_NAMES` and, for the names an OS changes ([`Provider::label_on`]),
    /// `PROVIDER_NAMES_ON` into `src/lib/tauri/generated/ProviderNames.ts`.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Virtualbox => "VirtualBox",
            Provider::VmwareDesktop => "VMware",
            Provider::Hyperv => "Hyper-V",
            Provider::Parallels => "Parallels",
            Provider::Libvirt => "libvirt",
            Provider::Qemu => "QEMU",
            Provider::Utm => "UTM",
            Provider::VmwareEsxi => "VMware ESXi",
            Provider::Proxmox => "Proxmox",
            Provider::Aws => "AWS",
            Provider::Azure => "Azure",
            Provider::Gcp => "Google Cloud",
            Provider::DigitalOcean => "DigitalOcean",
            Provider::Linode => "Linode",
            Provider::Oci => "Oracle Cloud",
        }
    }

    /// Remote providers run VMs on another host and need connection settings.
    pub fn is_remote(self) -> bool {
        Provider::SERVERS.contains(&self) || Provider::CLOUDS.contains(&self)
    }

    /// Whether it runs VMs built for another CPU (emulated, slowly): QEMU runs an x86 lab on an
    /// Apple Silicon Mac. The webview reads them as `EMULATORS` (`testing/ts_bindings.rs`).
    pub fn is_emulator(self) -> bool {
        self == Provider::Qemu
    }

    /// Cloud accounts (billed to the player), as opposed to the player's own servers.
    pub fn is_cloud(self) -> bool {
        Self::CLOUDS.contains(&self)
    }

    /// Whether this provider can run on the current OS at all. Local hypervisors are
    /// platform-specific; remote ones (ESXi, Proxmox) apply anywhere. We only surface
    /// providers that are possible on this machine, not the whole catalogue.
    fn applicable(self) -> bool {
        self.applicable_on(std::env::consts::OS, std::env::consts::ARCH)
    }

    /// [`applicable`](Self::applicable) on `os` and CPU `arch`.
    fn applicable_on(self, os: &str, arch: &str) -> bool {
        // Neither ships for Windows on Arm (no installer, no Arm VMs there): VM labs on those PCs
        // go to Hyper-V, a server or the cloud.
        let built_for_windows_on_arm = !(matches!(self, Provider::Virtualbox | Provider::VmwareDesktop) && os == "windows" && arch == "aarch64");
        self.runs_on(os) && built_for_windows_on_arm
    }

    /// Whether it exists on `os` (`std::env::consts::OS`) at all: Hyper-V on Windows, Parallels
    /// and UTM on macOS, libvirt on Linux, QEMU off Windows, every other one anywhere. The one table of which OS a
    /// hypervisor belongs to: the setup's installs and removals read it too.
    pub fn runs_on(self, os: &str) -> bool {
        match self {
            Provider::Hyperv => os == "windows",
            Provider::Parallels | Provider::Utm => os == "macos",
            Provider::Libvirt => os == "linux",
            // vagrant-qemu builds a networked VM's cloud-init seed with `oscdimg` on Windows (the
            // Windows ADK's), and Isoloom gives the qemu provider Linux and macOS hosts only.
            Provider::Qemu => os != "windows",
            _ => true,
        }
    }

    /// Whether it is offered on this machine for a lab's VMs: applicable here, and, on a Mac, the
    /// hypervisor made for its CPU (UTM on Apple Silicon, VirtualBox on an Intel Mac). One already
    /// `installed` is offered wherever it applies: the player chose it.
    fn offered(self, installed: bool) -> bool {
        self.offered_on(std::env::consts::OS, std::env::consts::ARCH, installed)
    }

    /// [`offered`](Self::offered) on `os` and CPU `arch`.
    fn offered_on(self, os: &str, arch: &str, installed: bool) -> bool {
        let apple_silicon = os == "macos" && arch == "aarch64";
        let for_this_mac = match self {
            Provider::Virtualbox => !apple_silicon,
            Provider::Utm => os != "macos" || apple_silicon,
            _ => true,
        };
        self.applicable_on(os, arch) && (installed || for_this_mac)
    }

    /// Vagrant plugin required, if the provider is not built in: Isoloom's table for the providers
    /// its Vagrantfiles drive ([`super::isothrum::vagrant_plugin`]), the launcher's own for
    /// Proxmox; the clouds aren't Vagrant providers.
    pub(crate) fn plugin(self) -> Option<&'static str> {
        match self {
            Provider::Proxmox => Some("vagrant-proxmox"),
            other => super::isothrum::vagrant_plugin(other),
        }
    }

    /// The oldest version of that plugin the launcher drives labs with: `vagrant_utm` below 0.2.0
    /// lacks the UTM 5 driver or the fixes labs are tested on, and `vagrant plugin install`
    /// (which setup offers again for an old one) updates it.
    fn plugin_minimum(self) -> Option<&'static str> {
        match self {
            Provider::Utm => Some("0.2.0"),
            _ => None,
        }
    }

    /// The hypervisor's name and the oldest version its plugin drives: `vagrant_utm` refuses a
    /// UTM before 4.6.5 (no import of a box, no registry for its shared folders).
    fn hypervisor_minimum(self) -> Option<(&'static str, &'static str)> {
        match self {
            Provider::Utm => Some(("UTM", "4.6.5")),
            _ => None,
        }
    }

    /// Whether Vagrant drives it through HashiCorp's Vagrant VMware Utility, a system service of
    /// its own (installed apart from the plugin, with an administrator's password) that the
    /// `vagrant-vmware-desktop` plugin asks to run VMware: without it every `vagrant up` fails.
    pub fn needs_vmware_utility(self) -> bool {
        self == Provider::VmwareDesktop
    }

    /// The hypervisor's own version, when it is installed and says it: UTM's app bundle
    /// (`CFBundleShortVersionString`); the others aren't held to a version yet.
    async fn hypervisor_version(self) -> Option<String> {
        if self != Provider::Utm {
            return None;
        }
        let plist = format!("{UTM_APP}/Contents/Info.plist");
        let out = run_read("defaults", &["read", &plist, "CFBundleShortVersionString"], None).await.ok()?;
        let version = out.trim().to_string();
        (!version.is_empty()).then_some(version)
    }

    /// Local tool that proves the hypervisor itself is installed.
    fn probe(self) -> Option<(&'static str, &'static [&'static str])> {
        match self {
            Provider::Virtualbox => Some(("VBoxManage", &["--version"])),
            Provider::VmwareDesktop => Some(("vmrun", &[])),
            Provider::Parallels => Some(("prlctl", &["--version"])),
            Provider::Libvirt => Some(("virsh", &["--version"])),
            Provider::Qemu => Some((qemu_binary(), &["--version"])),
            // Hyper-V is a Windows feature, UTM is checked by its app bundle, and
            // remote providers have nothing local to probe.
            Provider::Hyperv
            | Provider::Utm
            | Provider::VmwareEsxi
            | Provider::Proxmox
            | Provider::Aws
            | Provider::Azure
            | Provider::Gcp
            | Provider::DigitalOcean
            | Provider::Linode
            | Provider::Oci => None,
        }
    }

    /// Whether the hypervisor itself is installed here. None when there's nothing local to check
    /// (remote providers). Hyper-V is a Windows feature: its management service (`vmms.exe`)
    /// exists once the feature is on, and Windows Home has none.
    async fn hypervisor_installed(self) -> Option<bool> {
        if self == Provider::Utm {
            return Some(std::path::Path::new(UTM_APP).exists());
        }
        if self == Provider::Hyperv {
            return Some(hyperv_enabled());
        }
        let (program, args) = self.probe()?;
        Some(tool_present(program, args).await)
    }
}

const UTM_APP: &str = "/Applications/UTM.app";

/// Whether the Hyper-V feature is on: its Virtual Machine Management service is installed with it.
fn hyperv_enabled() -> bool {
    let root = std::env::var_os("SystemRoot").map_or_else(|| std::path::PathBuf::from("C:\\Windows"), std::path::PathBuf::from);
    root.join("System32").join("vmms.exe").is_file()
}

fn qemu_binary() -> &'static str {
    if std::env::consts::ARCH == "aarch64" { "qemu-system-aarch64" } else { "qemu-system-x86_64" }
}

/// Whether `provider`'s VMs need `mkisofs` on `os`: QEMU's on Linux. vagrant-qemu gives a VM on a
/// lab network (every lab VM on QEMU, through its network's hub) its settings in a cloud-init
/// seed, an ISO Vagrant's `create_iso` builds, with `mkisofs` on Linux (`hdiutil` on macOS,
/// which every Mac has). Without it, `vagrant up` fails at the lab's first VM.
pub(crate) fn needs_mkisofs(provider: Provider, os: &str) -> bool {
    provider == Provider::Qemu && os == "linux"
}

/// What the machine check found for one provider: whether a lab's VMs can run on it here, and
/// what is still missing.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    /// The provider.
    pub provider: Provider,
    /// A server, run elsewhere: nothing of it is installed here.
    pub remote: bool,
    /// Whether a lab's VMs can run on it now (`reason` says why not).
    pub available: bool,
    /// Whether the hypervisor itself is installed (for Hyper-V, whether the Windows feature is
    /// on; for QEMU on Linux, with the `mkisofs` its VMs need, [`needs_mkisofs`]). None where
    /// nothing local is checked: a server or a cloud.
    pub hypervisor: Option<bool>,
    /// The Vagrant plugin this provider needs, if it is not built in.
    pub plugin: Option<String>,
    /// Whether that plugin is present at a version the launcher drives (true when the provider is
    /// built in): an older one reads as not installed, so setup installs it again, which updates it.
    pub plugin_installed: bool,
    /// Where HashiCorp's Vagrant VMware Utility stands; `None` when the provider doesn't use it.
    pub utility: Option<UtilityState>,
    /// What is still to install for Vagrant to drive it here ([`needs`]): empty when nothing is.
    /// The Machine page and the setup steps read this, never their own idea of it.
    pub needs: Vec<Need>,
    /// Why it is not available (Vagrant, the hypervisor or its plugin missing or too old, the
    /// account not allowed to drive it, …), as a code the webview says in the player's language.
    pub reason: Option<Unavailable>,
    /// What this account lacks to use it although it is installed (the right to manage Hyper-V,
    /// a new login after joining libvirt's groups), whatever else is missing: the Machine page
    /// offers the way out by it.
    #[ts(optional = nullable)]
    pub account_lacks: Option<AccountLacks>,
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
    let mkisofs_missing = found == Some(true) && needs_mkisofs(provider, std::env::consts::OS) && !tool_present("mkisofs", &["-version"]).await;
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
    let joined = login_groups(provider, std::env::consts::OS);
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

/// What the player's account lacks to use a provider installed here.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AccountLacks {
    /// The right to manage Hyper-V (Hyper-V only).
    HypervAdmin,
    /// A new login: it joined libvirt's groups, or `/dev/kvm`'s (QEMU on Linux), since logging
    /// in.
    NewLogin,
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
    } else if let Some(why) = uses_kvm(provider, std::env::consts::OS).then(kvm_problem).flatten() {
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
