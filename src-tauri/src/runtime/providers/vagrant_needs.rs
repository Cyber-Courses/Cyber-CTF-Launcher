//! What Vagrant needs on this machine to drive a provider, beyond the hypervisor itself: Vagrant,
//! the provider's plugin, and for VMware's desktop hypervisors HashiCorp's Vagrant VMware Utility
//! (a system service of its own). [`needs`] is the one list readiness ([`super::detect`]) and the
//! setup steps both read.

use serde::Serialize;
use ts_rs::TS;

/// What is still missing on this machine for Vagrant to drive a provider, in the order setup
/// installs it. The one rule both sides read: a provider is ready only when this list is empty
/// (and its hypervisor runs), and the setup steps offer an install for each entry, so finishing
/// them can't leave it short of something no step installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Need {
    /// Vagrant itself.
    Vagrant,
    /// The provider's Vagrant plugin ([`super::ProviderStatus::plugin`]), or a newer one.
    Plugin,
    /// HashiCorp's Vagrant VMware Utility, installed and its service running.
    VmwareUtility,
}

/// Where HashiCorp's Vagrant VMware Utility stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum UtilityState {
    /// Not installed.
    Missing,
    /// Installed, but its service doesn't answer.
    Stopped,
    /// Installed, its service answering: what the plugin needs.
    Running,
}

/// What is missing for Vagrant to drive a provider here, from whether Vagrant is installed, its
/// plugin is (true when it needs none), and where the VMware Utility stands (`None` when it needs
/// none). See [`Need`].
pub fn needs(vagrant_installed: bool, plugin_installed: bool, utility: Option<UtilityState>) -> Vec<Need> {
    let mut needs = Vec::new();
    if !vagrant_installed {
        needs.push(Need::Vagrant);
    }
    if !plugin_installed {
        needs.push(Need::Plugin);
    }
    if utility.is_some_and(|u| u != UtilityState::Running) {
        needs.push(Need::VmwareUtility);
    }
    needs
}

/// The port the Vagrant VMware Utility's service listens on, on this machine only: the plugin's
/// default (`utility_port`), which Isoloom's Vagrantfiles keep.
pub const VMWARE_UTILITY_PORT: u16 = 9922;

/// What proves the Vagrant VMware Utility is installed: its program on macOS and Linux (HashiCorp's
/// package puts it under `/opt/vagrant-vmware-desktop`), and on Windows the certificates its
/// installer generates (under `%ProgramData%`), wherever the program went.
fn vmware_utility_marker() -> std::path::PathBuf {
    if cfg!(windows) {
        let data = std::env::var_os("ProgramData").map_or_else(|| std::path::PathBuf::from("C:\\ProgramData"), std::path::PathBuf::from);
        data.join("HashiCorp").join("vagrant-vmware-desktop").join("certificates")
    } else {
        std::path::PathBuf::from("/opt/vagrant-vmware-desktop/bin/vagrant-vmware-utility")
    }
}

/// Where the Vagrant VMware Utility stands here: running when its service answers on
/// [`VMWARE_UTILITY_PORT`] (what the plugin talks to, whatever installed it), else installed or
/// not by [`vmware_utility_marker`].
pub async fn vmware_utility() -> UtilityState {
    // Unit tests read the one they set, never this machine's; the guided setup's integration test
    // (its report knob set: `integration_tests::setup`) reads this machine's, as the app does.
    if cfg!(test) && std::env::var_os("CYBERCTF_TEST_SETUP_REPORT").is_none() {
        return fake_utility::state();
    }
    probe_vmware_utility().await
}

/// [`vmware_utility`] on this machine.
async fn probe_vmware_utility() -> UtilityState {
    let answers = tokio::time::timeout(std::time::Duration::from_secs(2), tokio::net::TcpStream::connect(("127.0.0.1", VMWARE_UTILITY_PORT))).await;
    if matches!(answers, Ok(Ok(_))) {
        UtilityState::Running
    } else if vmware_utility_marker().exists() {
        UtilityState::Stopped
    } else {
        UtilityState::Missing
    }
}

/// Unit tests' Vagrant VMware Utility: missing unless a test sets it (never this machine's own).
pub(super) mod fake_utility {
    use std::cell::Cell;

    use super::UtilityState;

    thread_local! {
        static STATE: Cell<UtilityState> = const { Cell::new(UtilityState::Missing) };
    }

    /// The utility a test set, else missing.
    pub(super) fn state() -> UtilityState {
        STATE.with(Cell::get)
    }

    /// Sets this thread's utility until the guard is dropped.
    #[cfg(test)]
    pub(crate) struct Utility;

    #[cfg(test)]
    impl Utility {
        pub(crate) fn set(state: UtilityState) -> Utility {
            STATE.with(|s| s.set(state));
            Utility
        }
    }

    #[cfg(test)]
    impl Drop for Utility {
        fn drop(&mut self) {
            STATE.with(|s| s.set(UtilityState::Missing));
        }
    }
}
