//! The hypervisors, servers and cloud accounts labs run on. The local hypervisors are Vagrant
//! providers: only what is usable on this machine is detected, and the runner is given the
//! provider. The servers and cloud accounts share the enum; labs run there through the Isothrum
//! runner (Proxmox and the clouds' `cloud-docker`, `cloud-vm` and `cloud-services` through
//! Terraform, ESXi through Vagrant), and they are never detected here.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::platform::exec::{run_read, timed_out};
use crate::platform::remembered::remembered;

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
        matches!(
            self,
            Provider::VmwareEsxi
                | Provider::Proxmox
                | Provider::Aws
                | Provider::Azure
                | Provider::Gcp
                | Provider::DigitalOcean
                | Provider::Linode
                | Provider::Oci
        )
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
        match self {
            // Neither ships for Windows on Arm (no installer, no Arm VMs there): VM labs on those
            // PCs go to Hyper-V, a server or the cloud.
            Provider::Virtualbox | Provider::VmwareDesktop if os == "windows" && arch == "aarch64" => false,
            Provider::Hyperv => os == "windows",
            Provider::Parallels | Provider::Utm => os == "macos",
            Provider::Libvirt => os == "linux",
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
        let not_for_this_mac = match self {
            Provider::Virtualbox => apple_silicon,
            Provider::Utm => os == "macos" && !apple_silicon,
            _ => false,
        };
        self.applicable_on(os, arch) && (installed || !not_for_this_mac)
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

mod vagrant_needs;
#[cfg(test)]
use vagrant_needs::fake_utility;
pub use vagrant_needs::{Need, UtilityState, VMWARE_UTILITY_MISSING, VMWARE_UTILITY_STOPPED, needs, vmware_utility};

/// Whether the Hyper-V feature is on: its Virtual Machine Management service is installed with it.
fn hyperv_enabled() -> bool {
    let root = std::env::var_os("SystemRoot").map_or_else(|| std::path::PathBuf::from("C:\\Windows"), std::path::PathBuf::from);
    root.join("System32").join("vmms.exe").is_file()
}

/// Why libvirt can't run VMs here although `virsh` is installed: vagrant-libvirt boots KVM
/// guests, so it needs `/dev/kvm`, and the player must be allowed to open it.
pub(crate) fn kvm_problem() -> Option<String> {
    let dev = std::path::Path::new("/dev/kvm");
    if !dev.exists() {
        return Some("KVM isn't available on this machine (no /dev/kvm): turn on virtualization in the firmware, or use another hypervisor".into());
    }
    if std::fs::OpenOptions::new().read(true).write(true).open(dev).is_err() {
        return Some("you can't use /dev/kvm yet: add yourself to the kvm and libvirt groups, then sign out and back in".into());
    }
    None
}

fn qemu_binary() -> &'static str {
    if std::env::consts::ARCH == "aarch64" { "qemu-system-aarch64" } else { "qemu-system-x86_64" }
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
    /// on). None where nothing local is checked: a server or a cloud.
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
    /// account not allowed to drive it, …).
    pub reason: Option<String>,
}

/// Each plugin of `vagrant plugin list` with its version, from lines like `vagrant-libvirt
/// (0.12.2, global)`; `None` for a line that names no version (a wrapped or cut line).
pub fn parse_plugin_versions(out: &str) -> Vec<(String, Option<String>)> {
    out.lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let name = words.next().filter(|name| name.starts_with("vagrant-") || name.starts_with("vagrant_"))?;
            let version = words
                .next()
                .map(|w| w.trim_matches(|c: char| matches!(c, '(' | ')' | ',')).to_string())
                .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()));
            Some((name.to_string(), version))
        })
        .collect()
}

/// Whether version `found` is `min` or later: dot-separated numbers compared in turn, a missing
/// one as 0 (`4.7` reads as `4.7.0`) and a part that doesn't start with a digit (`beta`) as 0.
pub fn version_at_least(found: &str, min: &str) -> bool {
    let numbers = |v: &str| -> Vec<u64> {
        v.trim().split('.').map(|part| part.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
    };
    let (found, min) = (numbers(found), numbers(min));
    for i in 0..found.len().max(min.len()) {
        let (a, b) = (found.get(i).copied().unwrap_or(0), min.get(i).copied().unwrap_or(0));
        if a != b {
            return a > b;
        }
    }
    true
}

/// The version `installed` of `provider`'s plugin when it is older than the one the launcher
/// drives (`plugin_minimum`), with that minimum; `None` when it is recent enough, unknown, or
/// the plugin has no minimum.
fn plugin_too_old(provider: Provider, installed: Option<&str>) -> Option<(&str, &'static str)> {
    let (version, min) = installed.zip(provider.plugin_minimum())?;
    (!version_at_least(version, min)).then_some((version, min))
}

/// The version `installed` of `provider`'s hypervisor when it is older than the one its plugin
/// drives (`hypervisor_minimum`), with the hypervisor's name and that minimum.
fn hypervisor_too_old(provider: Provider, installed: Option<&str>) -> Option<(&'static str, &str, &'static str)> {
    let (version, (name, min)) = installed.zip(provider.hypervisor_minimum())?;
    (!version_at_least(version, min)).then_some((name, version, min))
}

async fn tool_present(program: &'static str, args: &[&str]) -> bool {
    match run_read(program, args, None).await {
        // vmrun without arguments prints usage and exits non-zero, but it exists.
        Ok(_) | Err(crate::error::Error::CommandFailed { .. }) => true,
        Err(_) => false,
    }
}

/// The Vagrant plugins installed, each with its version when it says one (none when `vagrant
/// plugin list` fails).
async fn installed_plugins() -> Vec<(String, Option<String>)> {
    remembered("vagrant plugin list", run_read("vagrant", &["plugin", "list"], None)).await.map(|o| parse_plugin_versions(&o)).unwrap_or_default()
}

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
    statuses.iter().filter(|p| !p.remote && p.available && p.hypervisor != Some(false)).map(|p| p.provider).collect()
}

/// What the machine check finds for each provider this machine can run (its OS and CPU), the
/// Vagrant plugins read only when Vagrant is installed (`vagrant_installed`).
pub async fn detect(vagrant_installed: bool) -> Vec<ProviderStatus> {
    let plugins = if vagrant_installed { installed_plugins().await } else { Vec::new() };
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
    let plugins = if vagrant_installed { installed_plugins().await } else { Vec::new() };
    detect_one(provider, vagrant_installed, &plugins).await
}

/// One provider's status, given whether Vagrant is installed and its `plugins`: none when it
/// isn't offered on this machine.
async fn detect_one(provider: Provider, vagrant_installed: bool, plugins: &[(String, Option<String>)]) -> Option<ProviderStatus> {
    // Only surface providers that can run on this machine, not the whole catalogue.
    if !provider.applicable() {
        return None;
    }

    // The hypervisor layer (the VM software itself).
    let hypervisor = provider.hypervisor_installed().await;
    if !provider.offered(hypervisor == Some(true)) {
        return None;
    }

    // The Vagrant layer (the plugin that drives this provider): present, and not older than
    // the one the launcher drives labs with.
    let plugin = provider.plugin().map(str::to_string);
    let found = plugin.as_deref().and_then(|p| plugins.iter().find(|(name, _)| name == p));
    let plugin_version = found.and_then(|(_, version)| version.clone());
    let plugin_installed = match &plugin {
        None => true, // built in to Vagrant (virtualbox, hyperv)
        Some(_) => vagrant_installed && found.is_some() && plugin_too_old(provider, plugin_version.as_deref()).is_none(),
    };
    // The hypervisor's own version, where its plugin is particular about it (UTM).
    let hypervisor_version = if hypervisor == Some(true) { provider.hypervisor_version().await } else { None };

    // Installed is not the same as working: `VBoxManage --version` never talks to VirtualBox's
    // service (VBoxSVC), which can wedge so that every real call hangs (a deploy then sits on
    // "Bringing machine up" forever). Ask it something that needs the service, briefly.
    let unresponsive = provider == Provider::Virtualbox
        && hypervisor == Some(true)
        && crate::platform::exec::run_env_timed("VBoxManage", &["list", "runningvms"], None, &[], VBOX_SERVICE_WAIT).await.is_err_and(|e| timed_out(&e));

    // Vagrant's Hyper-V provider refuses every command (status reads too) from an account
    // that may not manage Hyper-V: said here, with the way out, not as a raw error mid-boot.
    let needs_admin = provider == Provider::Hyperv && hypervisor == Some(true) && !crate::platform::exec::may_manage_hyperv();

    // libvirt's socket and /dev/kvm are its group's: added to them since logging in (the libvirt
    // install does it), the player can't use them before logging in again.
    let relogin = provider == Provider::Libvirt
        && hypervisor == Some(true)
        && crate::platform::exec::groups_pending().await.iter().any(|g| LIBVIRT_GROUPS.contains(&g.as_str()));

    // VMware's plugin drives VMware through HashiCorp's utility service, installed on its own.
    let utility = if provider.needs_vmware_utility() { Some(vmware_utility().await) } else { None };

    let detection = Detection {
        unresponsive,
        hypervisor,
        hypervisor_version: hypervisor_version.as_deref(),
        plugin: plugin.as_deref(),
        plugin_installed,
        plugin_version: plugin_version.as_deref(),
        utility,
        account: if needs_admin {
            Some(AccountLacks::HypervAdmin)
        } else if relogin {
            Some(AccountLacks::NewLogin)
        } else {
            None
        },
    };
    let reason = unavailable_reason(provider, vagrant_installed, detection);
    let needs = needs(vagrant_installed, plugin_installed, utility);

    Some(ProviderStatus { provider, remote: provider.is_remote(), available: reason.is_none(), hypervisor, plugin, plugin_installed, utility, needs, reason })
}

/// The groups libvirt's socket and `/dev/kvm` are opened through.
pub const LIBVIRT_GROUPS: [&str; 2] = ["libvirt", "kvm"];

/// Why libvirt can't be used yet: the player joined its groups since logging in.
pub const LIBVIRT_RELOGIN: &str = "you were added to the libvirt group since you logged in: log out and back in (or restart) for it to apply";

/// Why Hyper-V can't be used by this account yet, and the two ways out.
pub const HYPERV_NEEDS_ADMIN: &str = "Vagrant drives Hyper-V only for an administrator or a member of Hyper-V Administrators: \
choose Allow my account (then sign out of Windows and back in), or run Cyber CTF as administrator";

/// What `detect` found out about one provider: the per-provider facts `unavailable_reason` reads
/// to name the first layer that is missing (the hypervisor, its plugin, an admin right). Grouped
/// so the check reads one value rather than a handful of flags that always travel together;
/// whether Vagrant is installed at all is global, so it stays a separate argument.
#[derive(Clone, Copy)]
struct Detection<'a> {
    /// Whether the hypervisor's background service is wedged (a probe call timed out).
    unresponsive: bool,
    /// Whether the hypervisor is installed; `None` when there is nothing local to probe.
    hypervisor: Option<bool>,
    /// The hypervisor's version, when it is installed and held to one (UTM).
    hypervisor_version: Option<&'a str>,
    /// The Vagrant plugin this provider needs, if it is not built in.
    plugin: Option<&'a str>,
    /// Whether that plugin is installed, at a version the launcher drives.
    plugin_installed: bool,
    /// That plugin's installed version, when it is present and says one.
    plugin_version: Option<&'a str>,
    /// Where the Vagrant VMware Utility stands; `None` when the provider doesn't use it.
    utility: Option<UtilityState>,
    /// What this account lacks to use it, if anything.
    account: Option<AccountLacks>,
}

/// What the player's account lacks to use a provider installed here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AccountLacks {
    /// The right to manage Hyper-V (Hyper-V only).
    HypervAdmin,
    /// A new login: it joined libvirt's groups since logging in (libvirt only).
    NewLogin,
}

/// Why a provider can't be used here (None: it can), from whether Vagrant is installed and what
/// `detect` found out about the provider: Vagrant missing first, then what is wrong with the
/// hypervisor itself (nothing setup installs mends that), then the first of its [`needs`]. So it
/// is `None` only when nothing is needed: readiness and the setup steps can't disagree.
fn unavailable_reason(provider: Provider, vagrant_installed: bool, detection: Detection<'_>) -> Option<String> {
    let Detection { unresponsive, hypervisor, hypervisor_version, plugin, plugin_installed, plugin_version, utility, account } = detection;
    let needs = needs(vagrant_installed, plugin_installed, utility);
    if needs.contains(&Need::Vagrant) {
        Some("Vagrant is not installed".to_string())
    } else if unresponsive {
        Some(
            "VirtualBox isn't responding (its background service is stuck). Quit VirtualBox and any VM windows, or restart the computer, then re-check."
                .to_string(),
        )
    } else if hypervisor == Some(false) && provider == Provider::Hyperv {
        Some("Hyper-V isn't turned on (Windows Features > Hyper-V), or this edition of Windows doesn't have it".to_string())
    } else if account == Some(AccountLacks::HypervAdmin) {
        Some(HYPERV_NEEDS_ADMIN.to_string())
    } else if hypervisor == Some(false) {
        provider.probe().map(|(program, _)| format!("`{program}` was not found")).or_else(|| Some("not installed".to_string()))
    } else if account == Some(AccountLacks::NewLogin) {
        Some(LIBVIRT_RELOGIN.to_string())
    } else if let Some(why) = (provider == Provider::Libvirt).then(kvm_problem).flatten() {
        Some(why)
    } else if let Some((name, version, min)) = hypervisor_too_old(provider, hypervisor_version) {
        Some(format!("{name} {version} is too old: {min} or later is needed. Update {name}, then re-check"))
    } else if let Some((version, min)) = plugin_too_old(provider, plugin_version) {
        Some(format!("Vagrant plugin `{}` {version} is too old: {min} or later is needed. Install it again to update it", plugin.unwrap_or("?")))
    } else {
        needs.first().map(|need| match need {
            Need::Vagrant => "Vagrant is not installed".to_string(),
            Need::Plugin => format!("Vagrant plugin `{}` is not installed", plugin.unwrap_or("?")),
            Need::VmwareUtility if utility == Some(UtilityState::Stopped) => VMWARE_UTILITY_STOPPED.to_string(),
            Need::VmwareUtility => VMWARE_UTILITY_MISSING.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin names alone, as the checks before versions mattered read them.
    fn parse_plugins(out: &str) -> Vec<String> {
        parse_plugin_versions(out).into_iter().map(|(name, _)| name).collect()
    }

    #[test]
    fn vmwares_desktop_hypervisor_is_named_for_the_os_and_nothing_else_is() {
        assert_eq!(Provider::VmwareDesktop.label_on("macos"), "VMware Fusion");
        assert_eq!(Provider::VmwareDesktop.label_on("windows"), "VMware Workstation");
        assert_eq!(Provider::VmwareDesktop.label_on("linux"), "VMware Workstation");
        assert_eq!(Provider::VmwareDesktop.label_on("freebsd"), "VMware");
        for p in Provider::ALL.iter().chain(&Provider::CLOUDS).filter(|p| **p != Provider::VmwareDesktop) {
            for os in ["macos", "linux", "windows"] {
                assert_eq!(p.label_on(os), p.label(), "{p:?} on {os}");
            }
        }
    }

    #[test]
    fn parses_vagrant_plugin_list() {
        let out = "vagrant-libvirt (0.12.2, global)\nvagrant_utm (0.1.3, global)\nvagrant-vmware-esxi (2.5.5, global)\n  - Version Constraint: > 0\n";
        assert_eq!(parse_plugins(out), vec!["vagrant-libvirt", "vagrant_utm", "vagrant-vmware-esxi"]);
    }

    #[test]
    fn plugin_versions_are_read_and_a_missing_one_is_none() {
        let out = "vagrant-libvirt (0.12.2, global)\nvagrant_utm (0.2.0, global)\n  - Version Constraint: > 0\nvagrant-vmware-esxi\nvagrant-qemu (0.7.0)\n";
        assert_eq!(
            parse_plugin_versions(out),
            [
                ("vagrant-libvirt".to_string(), Some("0.12.2".to_string())),
                ("vagrant_utm".to_string(), Some("0.2.0".to_string())),
                ("vagrant-vmware-esxi".to_string(), None),
                ("vagrant-qemu".to_string(), Some("0.7.0".to_string())),
            ]
        );
    }

    #[test]
    fn versions_compare_number_by_number() {
        assert!(version_at_least("0.2.0", "0.2.0"));
        assert!(version_at_least("0.10.0", "0.2.0"), "10 > 2, not \"1\" < \"2\"");
        assert!(version_at_least("4.7", "4.6.5"), "a missing part reads as 0");
        assert!(version_at_least("5.0.6", "4.6.5"));
        assert!(!version_at_least("0.1.6", "0.2.0"));
        assert!(!version_at_least("4.5.1", "4.6.5"));
        assert!(!version_at_least("0.2.0.beta", "0.2.0.1"), "a word reads as 0");
        assert!(version_at_least("", "0") && !version_at_least("", "0.1"));
    }

    #[test]
    fn an_old_utm_plugin_or_utm_reads_as_too_old_and_says_what_to_do() {
        let detection = |hypervisor_version, plugin_version, plugin_installed| Detection {
            unresponsive: false,
            hypervisor: Some(true),
            hypervisor_version,
            plugin: Some("vagrant_utm"),
            plugin_installed,
            plugin_version,
            utility: None,
            account: None,
        };
        // The plugin the failure was seen with: setup installs it again, which updates it.
        assert_eq!(plugin_too_old(Provider::Utm, Some("0.1.6")), Some(("0.1.6", "0.2.0")));
        assert_eq!(
            unavailable_reason(Provider::Utm, true, detection(Some("4.7.5"), Some("0.1.6"), false)).as_deref(),
            Some("Vagrant plugin `vagrant_utm` 0.1.6 is too old: 0.2.0 or later is needed. Install it again to update it")
        );
        // A UTM the plugin refuses, said before the plugin (updating the plugin wouldn't help).
        assert_eq!(
            unavailable_reason(Provider::Utm, true, detection(Some("4.5.1"), Some("0.1.6"), false)).as_deref(),
            Some("UTM 4.5.1 is too old: 4.6.5 or later is needed. Update UTM, then re-check")
        );
        // Recent enough, or no version to judge: nothing stands in the way.
        assert_eq!(unavailable_reason(Provider::Utm, true, detection(Some("4.7.5"), Some("0.2.0"), true)), None);
        assert_eq!(unavailable_reason(Provider::Utm, true, detection(None, None, true)), None);
        assert_eq!(plugin_too_old(Provider::Utm, None), None);
        // Other providers aren't held to a version.
        assert_eq!(plugin_too_old(Provider::Qemu, Some("0.1.0")), None);
        assert_eq!(hypervisor_too_old(Provider::Virtualbox, Some("6.0")), None);
    }

    #[test]
    fn provider_ids_match_serde_names() {
        for p in Provider::ALL {
            assert_eq!(serde_json::to_value(p).unwrap(), p.id());
        }
    }

    #[test]
    fn parse_plugins_handles_blank_versions_and_indentation() {
        // Built-in-only output, trailing detail lines, blank lines, and non-plugin noise.
        let out = "\nvagrant-vmware-desktop (3.0.4, global)\n  - Version Constraint: > 0\nvagrant-qemu (0.3.3)\nsomething-else (1.0.0)\n\n";
        assert_eq!(parse_plugins(out), vec!["vagrant-vmware-desktop", "vagrant-qemu"]);
        assert_eq!(parse_plugins(""), Vec::<String>::new());
        assert_eq!(parse_plugins("No plugins installed.\n"), Vec::<String>::new());
    }

    #[test]
    fn plugin_lists_with_windows_line_ends_indentation_and_broken_lines_still_name_the_plugins() {
        // CRLF (Windows), an indented entry, a line that is only a version in brackets (a wrapped
        // or cut line), and an entry with no version at all: the plugins are still found, and
        // no name carries a `\r` that would then never match.
        let out = "vagrant-libvirt (0.12.2, global)\r\n  vagrant_utm (0.1.3, global)\r\n(2.5.5, global)\r\n  - Version Constraint: > 0\r\nvagrant-vmware-esxi\r\n\r\n";
        assert_eq!(parse_plugins(out), ["vagrant-libvirt", "vagrant_utm", "vagrant-vmware-esxi"]);
        // A version glued to the name is not how Vagrant prints, and is not a known plugin.
        assert_eq!(parse_plugins("vagrant-qemu(0.3.3)\n"), ["vagrant-qemu(0.3.3)"]);
    }

    #[test]
    fn parse_plugins_accepts_both_hyphen_and_underscore_prefixes() {
        let out = "vagrant-libvirt (0.12.2)\nvagrant_utm (0.1.3)\n";
        assert_eq!(parse_plugins(out), vec!["vagrant-libvirt", "vagrant_utm"]);
    }

    #[test]
    fn all_detectable_providers_exclude_clouds() {
        // ALL is the locally detectable catalogue; cloud and AWS-family are never detected.
        for p in Provider::ALL {
            assert!(!matches!(p, Provider::Aws | Provider::Azure | Provider::Gcp | Provider::DigitalOcean | Provider::Linode | Provider::Oci));
        }
        assert_eq!(Provider::ALL.len(), 9);
    }

    #[test]
    fn remote_and_cloud_classification() {
        // Servers are remote but not cloud; the billed accounts are both.
        for server in Provider::SERVERS {
            assert!(server.is_remote() && !server.is_cloud());
        }
        for cloud in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
            assert!(cloud.is_remote() && cloud.is_cloud());
        }
        // Local hypervisors are neither.
        for local in [Provider::Virtualbox, Provider::Hyperv, Provider::Parallels, Provider::Libvirt, Provider::Qemu, Provider::Utm] {
            assert!(!local.is_remote() && !local.is_cloud());
        }
    }

    #[test]
    fn built_in_providers_need_no_plugin_and_clouds_have_none() {
        assert_eq!(Provider::Virtualbox.plugin(), None);
        assert_eq!(Provider::Hyperv.plugin(), None);
        // Each non-builtin local/remote-hypervisor provider names a plugin.
        for p in [Provider::VmwareDesktop, Provider::Parallels, Provider::Libvirt, Provider::Qemu, Provider::Utm, Provider::VmwareEsxi, Provider::Proxmox] {
            assert!(p.plugin().is_some(), "{p:?}");
        }
        // Cloud providers are not Vagrant providers, so no plugin.
        for p in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
            assert_eq!(p.plugin(), None, "{p:?}");
        }
    }

    #[test]
    fn ids_are_unique_and_round_trip_through_serde() {
        let all = [
            Provider::Virtualbox,
            Provider::VmwareDesktop,
            Provider::Hyperv,
            Provider::Parallels,
            Provider::Libvirt,
            Provider::Qemu,
            Provider::Utm,
            Provider::VmwareEsxi,
            Provider::Proxmox,
            Provider::Aws,
            Provider::Azure,
            Provider::Gcp,
            Provider::DigitalOcean,
            Provider::Linode,
            Provider::Oci,
        ];
        let mut ids: Vec<&str> = all.iter().map(|p| p.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "provider ids must be unique");
        // id() is the serde wire name, and it round-trips back to the same provider.
        for p in all {
            let round: Provider = serde_json::from_value(serde_json::Value::String(p.id().to_string())).unwrap();
            assert_eq!(round, p);
        }
    }

    #[test]
    fn applicability_follows_the_os() {
        use std::env::consts::OS;
        assert_eq!(Provider::Hyperv.applicable(), OS == "windows");
        assert_eq!(Provider::Parallels.applicable(), OS == "macos");
        assert_eq!(Provider::Utm.applicable(), OS == "macos");
        assert_eq!(Provider::Libvirt.applicable(), OS == "linux");
        for remote in [Provider::VmwareEsxi, Provider::Proxmox, Provider::Qemu, Provider::Aws] {
            assert!(remote.applicable(), "{remote:?}");
        }
    }

    #[test]
    fn a_mac_is_offered_the_hypervisor_made_for_its_cpu_and_keeps_one_installed() {
        // Was the webview's rule alone (`usableHypervisors`): the backend listed both, and offered
        // to install VirtualBox on Apple Silicon.
        assert!(!Provider::Virtualbox.offered_on("macos", "aarch64", false));
        assert!(Provider::Utm.offered_on("macos", "aarch64", false));
        assert!(Provider::Virtualbox.offered_on("macos", "x86_64", false));
        assert!(!Provider::Utm.offered_on("macos", "x86_64", false));
        assert!(Provider::Virtualbox.offered_on("macos", "aarch64", true), "installed: the player chose it");
        assert!(Provider::Virtualbox.offered_on("linux", "aarch64", false));
        assert!(!Provider::Utm.offered_on("linux", "aarch64", true), "never where it doesn't apply");
        assert!(!Provider::Hyperv.offered_on("macos", "aarch64", true));
    }

    #[test]
    fn probes_name_the_hypervisors_tool() {
        assert_eq!(Provider::Virtualbox.probe().unwrap().0, "VBoxManage");
        assert_eq!(Provider::VmwareDesktop.probe().unwrap().0, "vmrun");
        assert_eq!(Provider::Parallels.probe().unwrap().0, "prlctl");
        assert_eq!(Provider::Libvirt.probe().unwrap().0, "virsh");
        assert!(Provider::Qemu.probe().unwrap().0.starts_with("qemu-system-"));
        assert_eq!(qemu_binary(), if std::env::consts::ARCH == "aarch64" { "qemu-system-aarch64" } else { "qemu-system-x86_64" });
        for none in [Provider::Hyperv, Provider::Utm, Provider::VmwareEsxi, Provider::Proxmox, Provider::Gcp, Provider::Oci] {
            assert!(none.probe().is_none(), "{none:?}");
        }
    }

    #[test]
    fn kvm_problems_are_explained() {
        // Whatever this machine has, an answer is either nothing or a sentence about /dev/kvm.
        if let Some(why) = kvm_problem() {
            assert!(why.contains("/dev/kvm"), "{why}");
        }
    }

    #[tokio::test]
    async fn virtualbox_reads_as_not_responding_only_when_the_apps_timeout_cut_it_short() {
        // Regression: any `VBoxManage list runningvms` failure whose text said "timed out" (the
        // tool's own words) read as VirtualBox's service being stuck.
        use crate::platform::exec::fake::{Tools, fail, ok};
        let reason = |runningvms: &'static str| async move {
            let _tools = Tools::new(move |program, args| match (program, args) {
                ("VBoxManage", ["list", "runningvms"]) => fail(runningvms),
                ("VBoxManage", _) => ok("7.1.4\n"),
                _ => ok(""),
            });
            detect(true).await.into_iter().find(|s| s.provider == Provider::Virtualbox).and_then(|s| s.reason).unwrap_or_default()
        };
        assert!(!reason("VBoxManage: error: the guest session timed out").await.contains("isn't responding"));
        assert!(reason("timed out after 8s").await.contains("isn't responding"));
    }

    #[test]
    fn the_first_missing_layer_is_the_reason() {
        let r = |p, vagrant, stuck, hv, plugin, installed| {
            unavailable_reason(
                p,
                vagrant,
                Detection {
                    unresponsive: stuck,
                    hypervisor: hv,
                    hypervisor_version: None,
                    plugin,
                    plugin_installed: installed,
                    plugin_version: None,
                    utility: None,
                    account: None,
                },
            )
        };
        let hyperv = |needs_admin: bool| Detection {
            unresponsive: false,
            hypervisor: Some(true),
            hypervisor_version: None,
            plugin: None,
            plugin_installed: true,
            plugin_version: None,
            utility: None,
            account: needs_admin.then_some(AccountLacks::HypervAdmin),
        };
        assert_eq!(unavailable_reason(Provider::Hyperv, true, hyperv(true)).as_deref(), Some(HYPERV_NEEDS_ADMIN));
        assert!(HYPERV_NEEDS_ADMIN.contains("Hyper-V Administrators") && HYPERV_NEEDS_ADMIN.contains("Allow my account"));
        assert_eq!(unavailable_reason(Provider::Hyperv, true, hyperv(false)), None);
        assert_eq!(r(Provider::Virtualbox, false, true, Some(false), None, false).as_deref(), Some("Vagrant is not installed"));
        assert!(r(Provider::Virtualbox, true, true, Some(true), None, true).unwrap().contains("isn't responding"));
        assert_eq!(r(Provider::Virtualbox, true, false, Some(false), None, true).as_deref(), Some("`VBoxManage` was not found"));
        assert!(r(Provider::Hyperv, true, false, Some(false), None, true).unwrap().starts_with("Hyper-V isn't turned on"));
        assert_eq!(r(Provider::Utm, true, false, Some(false), Some("vagrant_utm"), true).as_deref(), Some("not installed"));
        assert_eq!(
            r(Provider::Parallels, true, false, Some(true), Some("vagrant-parallels"), false).as_deref(),
            Some("Vagrant plugin `vagrant-parallels` is not installed")
        );
        assert_eq!(r(Provider::Virtualbox, true, false, Some(true), None, true), None);
        assert_eq!(r(Provider::VmwareEsxi, true, false, None, Some("vagrant-vmware-esxi"), true), None);
        // libvirt also needs a usable /dev/kvm.
        assert_eq!(r(Provider::Libvirt, true, false, Some(true), Some("vagrant-libvirt"), true), kvm_problem());
    }

    #[test]
    fn libvirt_joined_since_logging_in_waits_for_the_next_login() {
        let libvirt = |relogin: bool| Detection {
            unresponsive: false,
            hypervisor: Some(true),
            hypervisor_version: None,
            plugin: Some("vagrant-libvirt"),
            plugin_installed: true,
            plugin_version: None,
            utility: None,
            account: relogin.then_some(AccountLacks::NewLogin),
        };
        assert_eq!(unavailable_reason(Provider::Libvirt, true, libvirt(true)).as_deref(), Some(LIBVIRT_RELOGIN));
        assert_eq!(unavailable_reason(Provider::Libvirt, true, libvirt(false)), kvm_problem());
        // Vagrant missing still comes first: the setup installs it in the same pass.
        assert_eq!(unavailable_reason(Provider::Libvirt, false, libvirt(true)).as_deref(), Some("Vagrant is not installed"));
    }

    /// A VMware detection: Fusion or Workstation installed, the plugin and the utility as given.
    fn vmware(plugin_installed: bool, utility: UtilityState) -> Detection<'static> {
        Detection {
            unresponsive: false,
            hypervisor: Some(true),
            hypervisor_version: None,
            plugin: Some("vagrant-vmware-desktop"),
            plugin_installed,
            plugin_version: None,
            utility: Some(utility),
            account: None,
        }
    }

    #[test]
    fn a_provider_is_ready_exactly_when_nothing_is_needed() {
        // The shared rule: readiness (`unavailable_reason`, what `available` and `ready` read) and
        // the setup steps (which install each of `needs`) agree on every combination, so a setup
        // that installs everything it lists leaves the provider ready, and one that is ready
        // lists nothing to install.
        for vagrant in [false, true] {
            for plugin_installed in [false, true] {
                for utility in [None, Some(UtilityState::Missing), Some(UtilityState::Stopped), Some(UtilityState::Running)] {
                    let detection = Detection { utility, ..vmware(plugin_installed, UtilityState::Running) };
                    let needs = needs(vagrant, plugin_installed, utility);
                    let reason = unavailable_reason(Provider::VmwareDesktop, vagrant, detection);
                    assert_eq!(reason.is_none(), needs.is_empty(), "vagrant {vagrant}, plugin {plugin_installed}, utility {utility:?}: {reason:?} / {needs:?}");
                }
            }
        }
        assert_eq!(needs(false, false, Some(UtilityState::Missing)), [Need::Vagrant, Need::Plugin, Need::VmwareUtility], "in setup's order");
    }

    #[test]
    fn vmware_needs_the_utility_running_as_well_as_its_plugin() {
        assert!(Provider::VmwareDesktop.needs_vmware_utility());
        assert!(Provider::ALL.iter().chain(&Provider::CLOUDS).filter(|p| **p != Provider::VmwareDesktop).all(|p| !p.needs_vmware_utility()));
        // The plugin alone isn't enough (the loop: setup put the plugin in, labs still couldn't run).
        assert_eq!(unavailable_reason(Provider::VmwareDesktop, true, vmware(true, UtilityState::Missing)).as_deref(), Some(VMWARE_UTILITY_MISSING));
        assert_eq!(unavailable_reason(Provider::VmwareDesktop, true, vmware(true, UtilityState::Stopped)).as_deref(), Some(VMWARE_UTILITY_STOPPED));
        // The plugin first: it is what setup installs first.
        assert_eq!(
            unavailable_reason(Provider::VmwareDesktop, true, vmware(false, UtilityState::Missing)).as_deref(),
            Some("Vagrant plugin `vagrant-vmware-desktop` is not installed")
        );
        assert_eq!(unavailable_reason(Provider::VmwareDesktop, true, vmware(true, UtilityState::Running)), None);
    }

    /// Fusion or Workstation installed (`vmrun` answers), Vagrant installed, with `plugins`.
    fn vmware_machine(plugins: &'static str) -> crate::platform::exec::fake::Tools {
        use crate::platform::exec::fake::{Tools, fail, ok};
        Tools::new(move |program, args| match (program, args) {
            ("vagrant", ["--version"]) => ok("Vagrant 2.4.9\n"),
            ("vagrant", ["plugin", "list"]) => ok(plugins),
            ("vmrun", _) => fail("usage: vmrun [AUTHENTICATION-FLAGS] COMMAND [PARAMETERS]"),
            _ => None,
        })
    }

    #[tokio::test]
    async fn a_vmware_machine_without_the_plugin_or_the_utility_needs_both_and_is_ready_once_both_are_in() {
        if !Provider::VmwareDesktop.applicable() {
            return; // Windows on Arm: no VMware Workstation there.
        }
        // This Mac's state when the loop was seen: Fusion installed, other plugins, neither of
        // VMware's two parts.
        let before = {
            let _tools = vmware_machine("vagrant-qemu (0.8.0, global)\nvagrant-vmware-esxi (2.5.5, global)\nvagrant_utm (0.2.0, global)\n");
            status(Provider::VmwareDesktop).await.unwrap()
        };
        assert_eq!(before.hypervisor, Some(true));
        assert_eq!(before.needs, [Need::Plugin, Need::VmwareUtility]);
        assert_eq!(before.utility, Some(UtilityState::Missing));
        assert!(!before.available);
        assert_eq!(ready(std::slice::from_ref(&before), true), Vec::<Provider>::new());

        // The plugin alone: still not ready, and what is left is said.
        let plugin_only = {
            let _tools = vmware_machine("vagrant-vmware-desktop (3.0.5, global)\n");
            status(Provider::VmwareDesktop).await.unwrap()
        };
        assert_eq!(plugin_only.needs, [Need::VmwareUtility]);
        assert_eq!(plugin_only.reason.as_deref(), Some(VMWARE_UTILITY_MISSING));
        assert_eq!(ready(std::slice::from_ref(&plugin_only), true), Vec::<Provider>::new());

        // Both in: ready, nothing left to install.
        let after = {
            let _tools = vmware_machine("vagrant-vmware-desktop (3.0.5, global)\n");
            let _utility = fake_utility::Utility::set(UtilityState::Running);
            status(Provider::VmwareDesktop).await.unwrap()
        };
        assert!(after.needs.is_empty() && after.available, "{:?}", after.reason);
        assert_eq!(ready(std::slice::from_ref(&after), true), [Provider::VmwareDesktop]);
    }

    #[test]
    fn statuses_serialize_in_camel_case() {
        let s = ProviderStatus {
            provider: Provider::Virtualbox,
            remote: false,
            available: true,
            hypervisor: Some(true),
            plugin: None,
            plugin_installed: true,
            utility: None,
            needs: vec![Need::Plugin, Need::VmwareUtility],
            reason: None,
        };
        let v = serde_json::to_value(s).unwrap();
        assert_eq!(v["provider"], "virtualbox");
        assert_eq!(v["pluginInstalled"], true);
        assert_eq!(v["needs"], serde_json::json!(["plugin", "vmwareUtility"]));
    }
}
