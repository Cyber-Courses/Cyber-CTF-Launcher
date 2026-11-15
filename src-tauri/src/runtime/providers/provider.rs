//! The places a lab's machines can run, and what each needs and offers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{hyperv_enabled, qemu_binary, tool_present, utm_app};
use crate::platform::exec::run_read;
use crate::platform::host::{Arch, Host, Os};

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

    /// The provider's name as the player reads it on `os`: VMware's desktop hypervisor is Fusion
    /// on macOS and Workstation elsewhere, so nobody is pointed at a product their OS doesn't have;
    /// every other provider is its [`Provider::label`].
    pub fn label_on(self, os: Os) -> &'static str {
        match (self, os) {
            (Provider::VmwareDesktop, Os::Macos) => "VMware Fusion",
            (Provider::VmwareDesktop, Os::Windows | Os::Linux) => "VMware Workstation",
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

    /// Its row of the hypervisor table: where it runs (which OS, which CPU), where its VMs share
    /// a folder of this computer and, when the setup doesn't install it, where its vendor offers
    /// it. Read by the machine check, the guided setup
    /// and the setup's installs and removals; how the setup installs and removes one is its tool's
    /// row (the machine layer's `Dependency::of_hypervisor`). A provider added to the enum is a compile error here until
    /// its row says where it runs.
    pub(crate) fn row(self) -> ProviderRow {
        // A hypervisor built for every OS and CPU, the setup installing it, its VMs sharing a
        // folder two-way; each row says how it differs.
        const EVERYWHERE: ProviderRow = ProviderRow { oses: &Os::ALL, windows_on_arm: true, mac_cpu: None, vendor: None, shares_folders: &Os::ALL };
        // A server or a cloud account: reached from any computer, sharing none of its folders.
        const REMOTE: ProviderRow = ProviderRow { shares_folders: &[], ..EVERYWHERE };
        match self {
            // Neither ships for Windows on Arm (no installer, no Arm VMs there): VM labs on those
            // PCs go to Hyper-V, a server or the cloud. VirtualBox runs a Mac's labs on Intel only.
            Provider::Virtualbox => ProviderRow { windows_on_arm: false, mac_cpu: Some(Arch::X86_64), ..EVERYWHERE },
            Provider::VmwareDesktop => {
                ProviderRow { windows_on_arm: false, vendor: Some("https://www.vmware.com/products/desktop-hypervisor/workstation-and-fusion"), ..EVERYWHERE }
            }
            // Hyper-V's SMB folders ask for a password at every start (Isoloom's `host_folders`).
            Provider::Hyperv => ProviderRow {
                oses: &[Os::Windows],
                vendor: Some("https://learn.microsoft.com/virtualization/hyper-v-on-windows/quick-start/enable-hyper-v"),
                shares_folders: &[],
                ..EVERYWHERE
            },
            Provider::Parallels => ProviderRow { oses: &[Os::Macos], vendor: Some("https://www.parallels.com/products/desktop/"), ..EVERYWHERE },
            // UTM is the hypervisor made for Apple silicon.
            Provider::Utm => ProviderRow { oses: &[Os::Macos], mac_cpu: Some(Arch::Aarch64), ..EVERYWHERE },
            Provider::Libvirt => ProviderRow { oses: &[Os::Linux], ..EVERYWHERE },
            // vagrant-qemu builds a networked VM's cloud-init seed with `oscdimg` on Windows (the
            // Windows ADK's), and Isoloom gives the qemu provider Linux and macOS hosts only; it
            // shares a folder two-way through virtiofsd, on Linux only.
            Provider::Qemu => ProviderRow { oses: &[Os::Macos, Os::Linux], shares_folders: &[Os::Linux], ..EVERYWHERE },
            Provider::VmwareEsxi
            | Provider::Proxmox
            | Provider::Aws
            | Provider::Azure
            | Provider::Gcp
            | Provider::DigitalOcean
            | Provider::Linode
            | Provider::Oci => REMOTE,
        }
    }

    /// Whether it can run on this computer at all. Local hypervisors are platform-specific; remote
    /// ones (ESXi, Proxmox) apply anywhere. Only providers possible on this machine are surfaced,
    /// not the whole catalogue.
    pub(super) fn applicable(self) -> bool {
        self.applicable_on(Host::here())
    }

    /// [`applicable`](Self::applicable) on `host`: its OS, and on Windows on Arm, built for it.
    fn applicable_on(self, host: Host) -> bool {
        self.runs_on(host.os) && (self.row().windows_on_arm || !host.is_windows_on_arm())
    }

    /// Whether it exists on `os` at all ([`Provider::row`]).
    pub fn runs_on(self, os: Os) -> bool {
        self.row().oses.contains(&os)
    }

    /// Whether it is offered on this machine for a lab's VMs: applicable here, and, on a Mac, the
    /// hypervisor made for its CPU (UTM on Apple Silicon, VirtualBox on an Intel Mac). One already
    /// `installed` is offered wherever it applies: the player chose it.
    pub(super) fn offered(self, installed: bool) -> bool {
        self.offered_on(Host::here(), installed)
    }

    /// [`offered`](Self::offered) on `host`.
    pub(crate) fn offered_on(self, host: Host, installed: bool) -> bool {
        let for_this_mac = host.os != Os::Macos || self.row().mac_cpu.is_none_or(|cpu| cpu == host.arch);
        self.applicable_on(host) && (installed || for_this_mac)
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
    pub(super) fn hypervisor_minimum(self) -> Option<HypervisorMinimum> {
        match self {
            Provider::Utm => Some(HypervisorMinimum { name: "UTM", version: "4.6.5" }),
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
        let plist = utm_app().join("Contents").join("Info.plist");
        let out = run_read("defaults", &["read", &plist.to_string_lossy(), "CFBundleShortVersionString"], None).await.ok()?;
        let version = out.trim().to_string();
        (!version.is_empty()).then_some(version)
    }

    /// Local tool that proves the hypervisor itself is installed.
    pub(super) fn probe(self) -> Option<Probe> {
        let probe = |program, args| Some(Probe { program, args });
        match self {
            Provider::Virtualbox => probe("VBoxManage", &["--version"]),
            Provider::VmwareDesktop => probe("vmrun", &[]),
            Provider::Parallels => probe("prlctl", &["--version"]),
            Provider::Libvirt => probe("virsh", &["--version"]),
            Provider::Qemu => probe(qemu_binary(), &["--version"]),
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
            return Some(utm_app().exists());
        }
        if self == Provider::Hyperv {
            return Some(hyperv_enabled());
        }
        let probe = self.probe()?;
        Some(tool_present(probe.program, probe.args).await)
    }
}

/// The local tool that proves a hypervisor is installed, and the arguments it is asked with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Probe {
    /// The program.
    pub(super) program: &'static str,
    /// Its arguments (none for `vmrun`, which prints its usage).
    pub(super) args: &'static [&'static str],
}

/// A hypervisor's name and the oldest version of it its plugin drives.
#[derive(Debug, Clone, Copy)]
pub(super) struct HypervisorMinimum {
    /// The hypervisor's name.
    pub(super) name: &'static str,
    /// The oldest version driven.
    pub(super) version: &'static str,
}

/// A provider's row of the hypervisor table ([`Provider::row`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderRow {
    /// The OSes it exists on.
    pub(crate) oses: &'static [Os],
    /// Whether it is built for Windows on Arm.
    pub(crate) windows_on_arm: bool,
    /// The Mac CPU it is made for, when it is made for one: offered on the other only once
    /// installed.
    pub(crate) mac_cpu: Option<Arch>,
    /// Where its vendor offers it, when the setup doesn't install it (behind a login, a licence,
    /// or a Windows feature the player turns on by hand).
    pub(crate) vendor: Option<&'static str>,
    /// The OSes where its VMs share a folder of this computer two-way.
    pub(crate) shares_folders: &'static [Os],
}
