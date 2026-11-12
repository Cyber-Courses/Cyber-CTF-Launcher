//! The versions the machine check reads: each Vagrant plugin's (`vagrant plugin list`), and
//! whether a plugin or a hypervisor is older than the launcher drives labs with.

use super::Provider;

/// Each plugin of `vagrant plugin list` with its version, from lines like `vagrant-libvirt
/// (0.12.2, global)`; `None` for a line that names no version (a wrapped or cut line).
pub(super) fn parse_plugin_versions(out: &str) -> Vec<(String, Option<String>)> {
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
pub(super) fn plugin_too_old(provider: Provider, installed: Option<&str>) -> Option<(&str, &'static str)> {
    let (version, min) = installed.zip(provider.plugin_minimum())?;
    (!version_at_least(version, min)).then_some((version, min))
}

/// The version `installed` of `provider`'s hypervisor when it is older than the one its plugin
/// drives (`hypervisor_minimum`), with the hypervisor's name and that minimum.
pub(super) fn hypervisor_too_old(provider: Provider, installed: Option<&str>) -> Option<(&'static str, &str, &'static str)> {
    let (version, (name, min)) = installed.zip(provider.hypervisor_minimum())?;
    (!version_at_least(version, min)).then_some((name, version, min))
}
