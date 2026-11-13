//! What the machine check reports for each provider.

use serde::Serialize;
use ts_rs::TS;

use super::{Need, Provider, Unavailable, UtilityState};

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
    /// on; for QEMU on Linux, with the `mkisofs` its VMs need, [`needs_mkisofs`](super::needs_mkisofs)). None where
    /// nothing local is checked: a server or a cloud.
    pub hypervisor: Option<bool>,
    /// The Vagrant plugin this provider needs, if it is not built in.
    pub plugin: Option<String>,
    /// Whether that plugin is present at a version the launcher drives (true when the provider is
    /// built in): an older one reads as not installed, so setup installs it again, which updates it.
    pub plugin_installed: bool,
    /// Where HashiCorp's Vagrant VMware Utility stands; `None` when the provider doesn't use it.
    pub utility: Option<UtilityState>,
    /// What is still to install for Vagrant to drive it here ([`needs`](super::needs)): empty when nothing is.
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
