//! Why a provider can't run a lab's VMs here, as a code the webview says in the player's language
//! (`machine.unavailable.*`, with the values its sentence needs), and its English sentence for the
//! log and the core's own messages.

use serde::Serialize;
use ts_rs::TS;

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
    pub fn message(&self) -> String {
        match self {
            Unavailable::VagrantMissing => "Vagrant is not installed.".into(),
            Unavailable::VirtualboxStuck => {
                "VirtualBox isn't responding (its background service is stuck). Quit VirtualBox and any VM windows, or restart the computer, then re-check.".into()
            }
            Unavailable::HypervOff => "Hyper-V isn't turned on (Windows Features > Hyper-V), or this edition of Windows doesn't have it.".into(),
            Unavailable::HypervNeedsAdmin => "Vagrant drives Hyper-V only for an administrator or a member of Hyper-V Administrators: choose Allow my account (then sign out of Windows and back in), or run Cyber CTF as administrator.".into(),
            Unavailable::MkisofsMissing => "QEMU VMs need mkisofs, which isn't installed: Vagrant builds with it the disk that gives each VM its lab network. Install QEMU again (that adds it), or run sudo apt-get install genisoimage, then re-check.".into(),
            Unavailable::NotInstalled { program: Some(program) } => format!("`{program}` was not found."),
            Unavailable::NotInstalled { program: None } => "Not installed.".into(),
            Unavailable::NewLogin { group } => format!("You were added to the {group} group since you logged in: log out and back in (or restart) for it to apply."),
            Unavailable::NoKvm => "KVM isn't available on this machine (no /dev/kvm): turn on virtualization in the firmware, or use another hypervisor.".into(),
            Unavailable::KvmDenied { group } => format!("You can't use /dev/kvm yet: add yourself to the {group} group, then sign out and back in."),
            Unavailable::HypervisorTooOld { name, version, min } => format!("{name} {version} is too old: {min} or later is needed. Update {name}, then re-check."),
            Unavailable::PluginTooOld { plugin, version, min } => {
                format!("Vagrant plugin `{plugin}` {version} is too old: {min} or later is needed. Install it again to update it.")
            }
            Unavailable::PluginMissing { plugin } => format!("Vagrant plugin `{plugin}` is not installed."),
            Unavailable::VmwareUtilityStopped => {
                "The Vagrant VMware Utility is installed, but its service isn't running: install it again (that restarts it), or restart the computer.".into()
            }
            Unavailable::VmwareUtilityMissing => "The Vagrant VMware Utility is not installed: HashiCorp's service the VMware plugin drives VMware through.".into(),
            Unavailable::VirtualboxDriver { kernel } => format!(
                "VirtualBox's kernel driver (vboxdrv) isn't loaded on this kernel ({kernel}), so VirtualBox can't start VMs. Install the kernel's headers (linux-headers-{kernel} on Debian and Ubuntu), build the driver (sudo /sbin/vboxconfig for Oracle's VirtualBox, sudo dpkg-reconfigure virtualbox-dkms for your distribution's), then re-check. With Secure Boot on, the driver must be signed before it loads."
            ),
            Unavailable::KernelRestart { kernel } => {
                format!("VirtualBox's kernel driver is built for the newer kernel installed with it ({kernel}): restart the computer to start that kernel, then re-check.")
            }
            Unavailable::KvmHoldsVirtualization => "KVM holds this computer's hardware virtualization (VT-x or AMD-V): Linux turns it on as soon as KVM's module loads (the default since Linux 6.12), so VirtualBox can't start VMs (\"VT-x is being used by another hypervisor\"). Use libvirt or QEMU, which run on KVM; for VirtualBox, unload KVM (sudo modprobe -r kvm_intel kvm_amd) or start Linux with kvm.enable_virt_at_load=0, then re-check.".into(),
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
            Unavailable::NotInstalled { program: Some(s("virsh")) },
            Unavailable::NotInstalled { program: None },
            Unavailable::NewLogin { group: s("libvirt") },
            Unavailable::NoKvm,
            Unavailable::KvmDenied { group: s("kvm") },
            Unavailable::HypervisorTooOld { name: s("UTM"), version: s("4.4.5"), min: s("4.5.0") },
            Unavailable::PluginTooOld { plugin: s("vagrant_utm"), version: s("0.1.0"), min: s("0.1.4") },
            Unavailable::PluginMissing { plugin: s("vagrant-libvirt") },
            Unavailable::VmwareUtilityStopped,
            Unavailable::VmwareUtilityMissing,
            Unavailable::VirtualboxDriver { kernel: s("6.12.86+deb13-amd64") },
            Unavailable::KernelRestart { kernel: s("6.12.116+deb13-amd64") },
            Unavailable::KvmHoldsVirtualization,
        ]
    }

    /// Each code's English sentence is said twice: here (the log, the setup's blocked step) and
    /// in the webview's catalogue (`machine.unavailable.*`, what the English UI shows). They must
    /// agree; a program not named is the catalogue's `notInstalledNoProgram`.
    #[test]
    fn every_reason_says_what_the_english_catalogue_says() {
        let catalogue: serde_json::Value = serde_json::from_str(include_str!("../../../../src/messages/en/machine.json")).unwrap();
        for reason in samples() {
            let wire = serde_json::to_value(&reason).unwrap();
            let mut key = wire["code"].as_str().unwrap().to_owned();
            if reason == (Unavailable::NotInstalled { program: None }) {
                key = "notInstalledNoProgram".into();
            }
            let mut text = catalogue["unavailable"][&key].as_str().unwrap_or_else(|| panic!("no machine.unavailable.{key}")).to_owned();
            for (name, value) in wire["args"].as_object().into_iter().flatten() {
                text = text.replace(&format!("{{{name}}}"), value.as_str().unwrap_or_default());
            }
            assert_eq!(reason.message(), text, "{wire}");
        }
    }
}
