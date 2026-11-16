//! Why a provider can't run a lab's VMs here, as a code the webview says in the player's language
//! (`machine.unavailable.*`, with the values its sentence needs), and its English sentence for the
//! log and the core's own messages.

use serde::Serialize;
use ts_rs::TS;

use crate::english;

/// Why a provider can't run a lab's VMs here: the first layer missing (Vagrant, the hypervisor,
/// the account's rights, its plugin, the VMware utility), as `detect` found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "code", content = "args", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Unavailable {
    /// Vagrant isn't installed.
    VagrantMissing,
    /// VirtualBox's background service is stuck: a call to it timed out.
    VirtualboxStuck,
    /// Hyper-V isn't turned on, or this edition of Windows doesn't have it.
    HypervOff,
    /// This account may not manage Hyper-V, which Vagrant needs (an administrator or a member of
    /// Hyper-V Administrators may).
    HypervNeedsAdmin,
    /// QEMU is installed, but not `mkisofs` (Linux), which Vagrant builds the cloud-init seed that
    /// gives a QEMU VM its lab network with: QEMU's install brings it (`genisoimage`).
    MkisofsMissing,
    /// libvirt or QEMU is installed, but no virtiofsd it finds (Linux), which it shares a folder of
    /// this computer with a VM through (the attack box's shared folder): its install brings it
    /// (`virtiofsd`).
    VirtiofsdMissing {
        /// The hypervisor's name ("libvirt", "QEMU").
        hypervisor: String,
    },
    /// The hypervisor isn't installed.
    NotInstalled {
        /// The program looked for, which wasn't found; None for a hypervisor found otherwise.
        program: Option<String>,
    },
    /// The player joined `group` since logging in (libvirt's, or the group `/dev/kvm` is opened
    /// through, for QEMU): it applies from the next login.
    NewLogin {
        /// The group joined.
        group: String,
    },
    /// This machine has no `/dev/kvm`: virtualization is off in the firmware, or there is none.
    NoKvm,
    /// `/dev/kvm` exists, but this account can't open it: it isn't in `group`, the one
    /// `/dev/kvm` belongs to (`kvm` on Debian, Ubuntu, Kali and openSUSE, whose `/dev/kvm` is
    /// `0660`; Fedora's and Arch's is `0666`, open to everyone).
    KvmDenied {
        /// The group `/dev/kvm` belongs to.
        group: String,
    },
    /// The hypervisor is older than its Vagrant plugin drives.
    HypervisorTooOld {
        /// The hypervisor's name.
        name: String,
        /// The version installed.
        version: String,
        /// The oldest version the plugin drives.
        min: String,
    },
    /// The Vagrant plugin is older than the launcher drives labs with.
    PluginTooOld {
        /// The plugin's name.
        plugin: String,
        /// The version installed.
        version: String,
        /// The oldest version the launcher drives.
        min: String,
    },
    /// The provider's Vagrant plugin isn't installed.
    PluginMissing {
        /// The plugin's name.
        plugin: String,
    },
    /// Whether the provider's Vagrant plugin is installed can't be told: `vagrant plugin list`
    /// failed (read as "not installed" before, the player was told to install a plugin that may
    /// be there).
    PluginsUnknown {
        /// The plugin's name.
        plugin: String,
        /// What `vagrant plugin list` said.
        reason: String,
    },
    /// The Vagrant VMware Utility is installed, but its service doesn't answer.
    VmwareUtilityStopped,
    /// The Vagrant VMware Utility, HashiCorp's service the VMware plugin drives VMware through,
    /// isn't installed.
    VmwareUtilityMissing,
    /// VirtualBox is installed, but its kernel driver (`vboxdrv`) isn't loaded on the running
    /// kernel (Linux), so no VM starts: never built (the kernel's headers missing), or refused
    /// (Secure Boot).
    VirtualboxDriver {
        /// The running kernel (`uname -r`).
        kernel: String,
    },
    /// VirtualBox's kernel driver waits for a restart (Linux): the package sources no longer have
    /// the running kernel's headers, so its install brought a newer kernel with its headers, which
    /// the driver is built for when the computer starts that kernel.
    KernelRestart {
        /// The newer kernel installed (`uname -r` once it runs).
        kernel: String,
    },
    /// KVM holds the CPU's virtualization extensions (Linux): its module (`kvm_intel`, `kvm_amd`)
    /// turns them on as it loads (`kvm.enable_virt_at_load`, the default since Linux 6.12), so no
    /// VirtualBox VM starts ("VT-x is being used by another hypervisor"), installed or not.
    KvmHoldsVirtualization,
}

impl Unavailable {
    /// The English sentence, for the log and the core's own messages; the webview says the code.
    /// It is the English catalogue's (`machine.unavailable.*`, what the English UI shows); a
    /// program not named is its `notInstalledNoProgram`.
    pub fn message(&self) -> String {
        let section = &english::MACHINE["unavailable"];
        match self {
            Unavailable::NotInstalled { program: None } => english::sentence(section, "notInstalledNoProgram", &serde_json::Value::Null),
            _ => english::coded(section, self),
        }
    }
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code, with sample values: what the webview's catalogue is held to.
    fn samples() -> Vec<Unavailable> {
        let s = String::from;
        vec![
            Unavailable::VagrantMissing,
            Unavailable::VirtualboxStuck,
            Unavailable::HypervOff,
            Unavailable::HypervNeedsAdmin,
            Unavailable::MkisofsMissing,
            Unavailable::VirtiofsdMissing { hypervisor: s("QEMU") },
            Unavailable::NotInstalled { program: Some(s("virsh")) },
            Unavailable::NotInstalled { program: None },
            Unavailable::NewLogin { group: s("libvirt") },
            Unavailable::NoKvm,
            Unavailable::KvmDenied { group: s("kvm") },
            Unavailable::HypervisorTooOld { name: s("UTM"), version: s("4.4.5"), min: s("4.5.0") },
            Unavailable::PluginTooOld { plugin: s("vagrant_utm"), version: s("0.1.0"), min: s("0.1.4") },
            Unavailable::PluginMissing { plugin: s("vagrant-libvirt") },
            Unavailable::PluginsUnknown { plugin: s("vagrant-libvirt"), reason: s("exited with code 1") },
            Unavailable::VmwareUtilityStopped,
            Unavailable::VmwareUtilityMissing,
            Unavailable::VirtualboxDriver { kernel: s("6.12.86+deb13-amd64") },
            Unavailable::KernelRestart { kernel: s("6.12.116+deb13-amd64") },
            Unavailable::KvmHoldsVirtualization,
        ]
    }

    /// Each code's English sentence is the English catalogue's, which every code has; filled in,
    /// no placeholder is left.
    #[test]
    fn every_reason_says_its_english_catalogue_sentence() {
        for reason in samples() {
            let wire = serde_json::to_value(&reason).unwrap();
            let text = reason.message();
            assert_ne!(text, wire["code"].as_str().unwrap(), "no sentence for {wire}");
            assert!(!text.contains('{'), "a placeholder left in {wire}: {text}");
        }
        let s = String::from;
        let too_old = Unavailable::HypervisorTooOld { name: s("UTM"), version: s("4.4.5"), min: s("4.5.0") };
        assert_eq!(too_old.message(), "UTM 4.4.5 is too old: 4.5.0 or later is needed. Update UTM, then re-check.");
        assert_eq!(Unavailable::NotInstalled { program: None }.message(), "Not installed.");
        assert_eq!(Unavailable::NotInstalled { program: Some(s("virsh")) }.message(), "`virsh` was not found.");
    }
}
