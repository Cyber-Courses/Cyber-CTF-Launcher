//! The places a lab's machines can run, and what each needs and offers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use isothrum_machine::providers::ProviderRow;

#[cfg(test)]
use isothrum_machine::Host;
use isothrum_machine::HostOs;

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

    /// The local hypervisors and the servers, not the clouds: the places the setup and the
    /// Machine page list (the machine check detects the Vagrant ones, [`detect`](super::detect)).
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
    pub fn label_on(self, os: HostOs) -> &'static str {
        match (self, os) {
            (Provider::VmwareDesktop, HostOs::Macos) => "VMware Fusion",
            (Provider::VmwareDesktop, HostOs::Windows | HostOs::Linux) => "VMware Workstation",
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

    /// The Vagrant provider it is (the runner's and isothrum-machine's, by the same id): a local
    /// hypervisor or ESXi. `None` for Proxmox and the clouds, which run through Terraform.
    pub fn vagrant(self) -> Option<isothrum_runner::Provider> {
        use isothrum_runner::Provider as V;
        Some(match self {
            Provider::Virtualbox => V::Virtualbox,
            Provider::VmwareDesktop => V::VmwareDesktop,
            Provider::Hyperv => V::Hyperv,
            Provider::Parallels => V::Parallels,
            Provider::Libvirt => V::Libvirt,
            Provider::Qemu => V::Qemu,
            Provider::Utm => V::Utm,
            Provider::VmwareEsxi => V::VmwareEsxi,
            Provider::Proxmox | Provider::Aws | Provider::Azure | Provider::Gcp | Provider::DigitalOcean | Provider::Linode | Provider::Oci => return None,
        })
    }

    /// Its row of isothrum-machine's hypervisor table, for a Vagrant provider ([`Provider::vagrant`]).
    fn row(self) -> Option<ProviderRow> {
        self.vagrant().map(ProviderRow::of)
    }

    /// Whether it exists on `os` at all: a hypervisor where its row says, a server or a cloud
    /// account everywhere.
    pub fn runs_on(self, os: HostOs) -> bool {
        self.row().is_none_or(|row| row.oses.contains(&os.isothrum()))
    }

    /// Where its vendor offers it, when the setup doesn't install it (behind a login, a licence,
    /// or a Windows feature the player turns on by hand).
    #[cfg(test)]
    pub(crate) fn vendor(self) -> Option<&'static str> {
        self.row()?.vendor
    }

    /// Whether its VMs share a folder of this computer (on `os`) two-way: never a server's or a
    /// cloud's.
    pub(crate) fn shares_folders_on(self, os: HostOs) -> bool {
        self.row().is_some_and(|row| row.shares_folders.contains(&os.isothrum()))
    }

    /// Whether it is offered on `host` for a lab's VMs (isothrum-machine's
    /// [`offered`](isothrum_machine::providers::offered)): applicable there, and, on a Mac, the
    /// hypervisor made for its CPU. One already `installed` is offered wherever it applies: the
    /// player chose it. A server or a cloud account is offered everywhere. (The machine check
    /// asks isothrum-machine; the setup's shapes ask this.)
    #[cfg(test)]
    pub(crate) fn offered_on(self, host: Host, installed: bool) -> bool {
        let Some(vagrant) = self.vagrant() else { return true };
        let machine = isothrum_machine::Host {
            os: host.os.isothrum(),
            arch: host.arch.isothrum(),
            packages: None,
            devices: isothrum_machine::Devices::here(host.os.isothrum()),
        };
        isothrum_machine::providers::offered(vagrant, &machine, installed)
    }

    /// The Vagrant plugin it needs, if it is a Vagrant provider not built in (the runner's table);
    /// the machine check's status names it.
    #[cfg(test)]
    pub(crate) fn plugin(self) -> Option<&'static str> {
        self.vagrant()?.plugin()
    }

    /// Whether Vagrant drives it through HashiCorp's Vagrant VMware Utility, a system service of
    /// its own (installed apart from the plugin, with an administrator's password).
    pub fn needs_vmware_utility(self) -> bool {
        self.vagrant().is_some_and(isothrum_machine::providers::needs_vmware_utility)
    }
}

impl From<isothrum_runner::Provider> for Provider {
    fn from(vagrant: isothrum_runner::Provider) -> Provider {
        use isothrum_runner::Provider as V;
        match vagrant {
            V::Virtualbox => Provider::Virtualbox,
            V::VmwareDesktop => Provider::VmwareDesktop,
            V::Hyperv => Provider::Hyperv,
            V::Parallels => Provider::Parallels,
            V::Libvirt => Provider::Libvirt,
            V::Qemu => Provider::Qemu,
            V::Utm => Provider::Utm,
            V::VmwareEsxi => Provider::VmwareEsxi,
        }
    }
}

/// A detected status's provider (isothrum-machine's, a Vagrant one) is the launcher's of the same
/// id.
impl PartialEq<Provider> for isothrum_runner::Provider {
    fn eq(&self, other: &Provider) -> bool {
        other.vagrant() == Some(*self)
    }
}

impl PartialEq<isothrum_runner::Provider> for Provider {
    fn eq(&self, other: &isothrum_runner::Provider) -> bool {
        other == self
    }
}
