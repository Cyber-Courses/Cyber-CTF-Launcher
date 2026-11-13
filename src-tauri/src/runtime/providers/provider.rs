//! The places a lab's machines can run, and what each needs and offers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{UTM_APP, hyperv_enabled, qemu_binary, tool_present};
use crate::platform::exec::run_read;

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
    /// considers ([`detect`](super::detect)).
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
    pub(super) fn applicable(self) -> bool {
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
    pub(super) fn offered(self, installed: bool) -> bool {
        self.offered_on(std::env::consts::OS, std::env::consts::ARCH, installed)
    }

    /// [`offered`](Self::offered) on `os` and CPU `arch`.
    pub(super) fn offered_on(self, os: &str, arch: &str, installed: bool) -> bool {
        let apple_silicon = os == "macos" && arch == "aarch64";
        let for_this_mac = match self {
            Provider::Virtualbox => !apple_silicon,
            Provider::Utm => os != "macos" || apple_silicon,
            _ => true,
        };
        self.applicable_on(os, arch) && (installed || for_this_mac)
    }

    /// Vagrant plugin required, if the provider is not built in: Isoloom's table for the providers
    /// its Vagrantfiles drive ([`crate::runtime::isothrum::vagrant_plugin`]), the launcher's own for
    /// Proxmox; the clouds aren't Vagrant providers.
    pub(crate) fn plugin(self) -> Option<&'static str> {
        match self {
            Provider::Proxmox => Some("vagrant-proxmox"),
            other => crate::runtime::isothrum::vagrant_plugin(other),
        }
    }

    /// The oldest version of that plugin the launcher drives labs with: `vagrant_utm` below 0.2.0
    /// lacks the UTM 5 driver or the fixes labs are tested on, and `vagrant plugin install`
    /// (which setup offers again for an old one) updates it.
    pub(super) fn plugin_minimum(self) -> Option<&'static str> {
        match self {
            Provider::Utm => Some("0.2.0"),
            _ => None,
        }
    }

    /// The hypervisor's name and the oldest version its plugin drives: `vagrant_utm` refuses a
    /// UTM before 4.6.5 (no import of a box, no registry for its shared folders).
    pub(super) fn hypervisor_minimum(self) -> Option<(&'static str, &'static str)> {
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
    pub(super) async fn hypervisor_version(self) -> Option<String> {
        if self != Provider::Utm {
            return None;
        }
        let plist = format!("{UTM_APP}/Contents/Info.plist");
        let out = run_read("defaults", &["read", &plist, "CFBundleShortVersionString"], None).await.ok()?;
        let version = out.trim().to_string();
        (!version.is_empty()).then_some(version)
    }

    /// Local tool that proves the hypervisor itself is installed.
    pub(super) fn probe(self) -> Option<(&'static str, &'static [&'static str])> {
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
    pub(super) async fn hypervisor_installed(self) -> Option<bool> {
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
