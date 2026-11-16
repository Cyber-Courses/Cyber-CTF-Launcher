//! What Vagrant prints, read: `vagrant plugin list`, and libvirt's refusal to start a VM without
//! a virtiofsd.

/// A Vagrant plugin `vagrant plugin list` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    /// Its name (`vagrant-libvirt`).
    pub name: String,
    /// Its version; `None` when its line names none whole.
    pub version: Option<String>,
}

/// Each plugin of `vagrant plugin list` with its version, from lines like `vagrant-libvirt
/// (0.12.2, global)`; `None` for a line that names no version whole (a wrapped or cut line).
#[must_use]
pub fn plugins(out: &str) -> Vec<Plugin> {
    out.lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let name = words.next().filter(|name| name.starts_with("vagrant-") || name.starts_with("vagrant_"))?;
            // `(0.12.2,` or `(0.12.2)`: a version word cut before its end (a cut last line) is
            // no version, not a shorter one.
            let version = words
                .next()
                .filter(|w| w.starts_with('(') && (w.ends_with(',') || w.ends_with(')')))
                .map(|w| w.trim_matches(|c: char| matches!(c, '(' | ')' | ',')).to_string())
                .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()));
            Some(Plugin { name: name.to_string(), version })
        })
        .collect()
}

/// libvirt's words when it starts a VM with a folder shared through virtiofs and finds no
/// virtiofsd (`virDomainCreate`), which vagrant-libvirt passes on in a Ruby stack trace.
const NO_VIRTIOFSD: &str = "Unable to find a satisfying virtiofsd";

/// Whether a VM's start failed for libvirt finding no virtiofsd, from what it printed (`said`).
#[must_use]
pub fn libvirt_lacks_virtiofsd(said: &str) -> bool {
    said.contains(NO_VIRTIOFSD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libvirt_finding_no_virtiofsd_is_read_from_vagrant_libvirts_stack_trace() {
        assert!(libvirt_lacks_virtiofsd(include_str!("fixtures/vagrant/libvirt-no-virtiofsd.txt")));
        assert!(!libvirt_lacks_virtiofsd("Call to virDomainCreate failed: Requested operation is not valid: network 'default' is not active"));
    }

    fn plugin(name: &str, version: Option<&str>) -> Plugin {
        Plugin { name: name.into(), version: version.map(str::to_owned) }
    }

    #[test]
    fn plugins_are_read_as_vagrant_lists_them() {
        let listed = plugins(include_str!("fixtures/vagrant/2.4.3-plugin-list.txt"));
        assert_eq!(listed, [plugin("vagrant-libvirt", Some("0.12.2")), plugin("vagrant-vmware-desktop", Some("3.0.4")), plugin("vagrant_utm", None),]);
        assert_eq!(plugins("No plugins installed."), Vec::<Plugin>::new());
    }
}
