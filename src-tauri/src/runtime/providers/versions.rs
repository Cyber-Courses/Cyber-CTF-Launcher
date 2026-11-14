//! The versions the machine check reads: each Vagrant plugin's (`vagrant plugin list`), and
//! whether a plugin or a hypervisor is older than the launcher drives labs with.

use super::Provider;

/// A version found older than the oldest one the launcher drives labs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TooOld<'a> {
    /// What is installed: the hypervisor's name, or none for a plugin (named by its provider).
    pub(super) name: Option<&'static str>,
    /// The version installed.
    pub(super) version: &'a str,
    /// The oldest version driven.
    pub(super) min: &'static str,
}

/// Whether version `found` is `min` or later: dot-separated numbers compared in turn, a missing
/// one as 0 (`4.7` reads as `4.7.0`) and a part that doesn't start with a digit (`beta`) as 0.
pub(super) fn version_at_least(found: &str, min: &str) -> bool {
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
pub(super) fn plugin_too_old(provider: Provider, installed: Option<&str>) -> Option<TooOld<'_>> {
    let (version, min) = installed.zip(provider.plugin_minimum())?;
    (!version_at_least(version, min)).then_some(TooOld { name: None, version, min })
}

/// The version `installed` of `provider`'s hypervisor when it is older than the one its plugin
/// drives (`hypervisor_minimum`), with the hypervisor's name and that minimum.
pub(super) fn hypervisor_too_old(provider: Provider, installed: Option<&str>) -> Option<TooOld<'_>> {
    let (version, minimum) = installed.zip(provider.hypervisor_minimum())?;
    (!version_at_least(version, minimum.version)).then_some(TooOld { name: Some(minimum.name), version, min: minimum.version })
}
