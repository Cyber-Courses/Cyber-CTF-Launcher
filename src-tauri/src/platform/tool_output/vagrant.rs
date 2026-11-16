//! What Vagrant prints, read: `vagrant plugin list`, and libvirt's or vagrant-qemu's refusal to
//! start a VM without a virtiofsd.

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
const LIBVIRT_NO_VIRTIOFSD: &str = "Unable to find a satisfying virtiofsd";

/// vagrant-qemu's words (0.7.0 on, `vagrant_qemu.errors.virtiofsd_not_found` in its `en.yml`)
/// when a `qemu_virtiofs` shared folder finds no virtiofsd where it looks.
const QEMU_NO_VIRTIOFSD: &str = "virtiofsd was not found or is not executable";

/// The hypervisor ("libvirt", "QEMU") a VM's start failed on for finding no virtiofsd, from what
/// it printed (`said`); none when it failed otherwise.
#[must_use]
pub fn lacks_virtiofsd(said: &str) -> Option<&'static str> {
    if said.contains(LIBVIRT_NO_VIRTIOFSD) {
        Some("libvirt")
    } else if said.contains(QEMU_NO_VIRTIOFSD) {
        Some("QEMU")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libvirt_finding_no_virtiofsd_is_read_from_vagrant_libvirts_stack_trace() {
        assert_eq!(lacks_virtiofsd(include_str!("fixtures/vagrant/libvirt-no-virtiofsd.txt")), Some("libvirt"));
        assert_eq!(lacks_virtiofsd("Call to virDomainCreate failed: Requested operation is not valid: network 'default' is not active"), None);
    }

    #[test]
    fn vagrant_qemu_finding_no_virtiofsd_is_read_from_its_error() {
        // vagrant-qemu's own sentence (its locale, 0.7.0 and 0.8.0); the line before it is
        // Vagrant's usual, not captured from a run.
        assert_eq!(lacks_virtiofsd(include_str!("fixtures/vagrant/qemu-no-virtiofsd.txt")), Some("QEMU"));
        // Its other virtiofsd failure (one found that didn't start) isn't a missing one.
        assert_eq!(lacks_virtiofsd("virtiofsd failed to start for /home/ada/CyberCTF/shared. Check the log at /tmp/x.log"), None);
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
