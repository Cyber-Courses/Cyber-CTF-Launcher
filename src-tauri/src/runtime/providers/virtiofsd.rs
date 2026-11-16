//! Whether libvirt finds a virtiofsd here (Linux). vagrant-libvirt shares a folder with a VM
//! (the attack box's shared folder) as a `virtiofs` filesystem, and libvirt starts the
//! `virtiofsd` a vhost-user descriptor names for it. With none, the VM's start fails deep in
//! vagrant-libvirt ("Unable to find a satisfying virtiofsd"), so a libvirt without one isn't ready.
//! virtiofsd is its own package from Debian 12, Ubuntu 23.10 and Fedora 36 on (QEMU's before), and
//! libvirt doesn't depend on it.

use std::path::{Path, PathBuf};

use super::Provider;
use crate::platform::host::Os;

/// Where libvirt reads QEMU's vhost-user descriptors, in the order it reads them: a descriptor in
/// a later folder takes the place of an earlier one of the same name (an administrator's, in
/// `/etc`, over the distribution's). libvirt's own system daemon (`qemu:///system`, which Vagrant
/// uses) reads no player's folder.
const VHOST_USER_DIRS: [&str; 2] = ["/usr/share/qemu/vhost-user", "/etc/qemu/vhost-user"];

/// Whether `provider`'s VMs need a virtiofsd on `os` for the folders this computer shares with
/// them: libvirt's (Linux only). QEMU's vagrant-qemu starts its own and isn't held to it here.
pub(crate) fn needs_virtiofsd(provider: Provider, os: Os) -> bool {
    provider == Provider::Libvirt && os == Os::Linux
}

/// Whether libvirt finds a virtiofsd on this machine ([`found_in`] its descriptor folders).
/// Always off Linux, where libvirt isn't offered, and in a test whose hypervisors are fakes.
pub(crate) fn found() -> bool {
    !cfg!(target_os = "linux") || super::kvm::devices_assumed_ready() || found_in(&VHOST_USER_DIRS).is_some()
}

/// The virtiofsd libvirt would start, from the vhost-user descriptors in `dirs` (read as libvirt
/// reads them, [`VHOST_USER_DIRS`]): the first, by file name, of type `fs` whose program is
/// there. An empty file hides the earlier folders' descriptor of its name, as libvirt reads it.
fn found_in(dirs: &[impl AsRef<Path>]) -> Option<PathBuf> {
    let mut descriptors = std::collections::BTreeMap::new();
    for dir in dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                descriptors.insert(entry.file_name(), path);
            }
        }
    }
    descriptors.values().find_map(|path| fs_binary(&std::fs::read_to_string(path).ok()?))
}

/// The program descriptor `json` names, when it is a virtiofsd's (`"type": "fs"`) and the program
/// is there.
fn fs_binary(json: &str) -> Option<PathBuf> {
    let descriptor: serde_json::Value = serde_json::from_str(json).ok()?;
    if descriptor["type"].as_str() != Some("fs") {
        return None;
    }
    let binary = PathBuf::from(descriptor["binary"].as_str()?);
    binary.is_file().then_some(binary)
}

/// Whether this computer may share a folder with a VM on `provider`: refused up front for libvirt
/// without a virtiofsd ([`found`]), whose start would otherwise fail deep in vagrant-libvirt.
pub(crate) fn can_share(provider: Provider) -> crate::error::Result<()> {
    share_on(provider, Os::HERE, found)
}

/// [`can_share`] on `os`, `found` telling whether libvirt finds a virtiofsd (asked only for
/// libvirt).
fn share_on(provider: Provider, os: Os, found: impl FnOnce() -> bool) -> crate::error::Result<()> {
    if needs_virtiofsd(provider, os) && !found() {
        return Err(crate::error::Error::coded(crate::error::Code::VirtiofsdMissing));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A descriptor naming `binary`, of `kind`.
    fn descriptor(kind: &str, binary: &Path) -> String {
        serde_json::json!({ "description": "virtiofsd", "type": kind, "binary": binary }).to_string()
    }

    #[test]
    fn libvirt_finds_the_virtiofsd_its_packages_descriptor_names() {
        // Debian 13's and Fedora's `virtiofsd` package: /usr/libexec/virtiofsd, named by
        // /usr/share/qemu/vhost-user/50-virtiofsd.json. Without it, the VM's start failed with
        // "Unable to find a satisfying virtiofsd".
        let root = crate::testing::temp_dir("virtiofsd");
        let (share, etc) = (root.join("share"), root.join("etc"));
        std::fs::create_dir_all(&share).unwrap();
        let binary = root.join("virtiofsd");
        assert_eq!(found_in(&[&share, &etc]), None, "no descriptor: none found");

        std::fs::write(share.join("50-virtiofsd.json"), descriptor("fs", &binary)).unwrap();
        assert_eq!(found_in(&[&share, &etc]), None, "a descriptor whose program is gone");
        std::fs::write(&binary, "").unwrap();
        assert_eq!(found_in(&[&share, &etc]), Some(binary.clone()));

        // An administrator's empty file of the same name hides it, as libvirt reads it.
        std::fs::create_dir_all(&etc).unwrap();
        std::fs::write(etc.join("50-virtiofsd.json"), "").unwrap();
        assert_eq!(found_in(&[&share, &etc]), None);
    }

    #[test]
    fn a_descriptor_of_another_vhost_user_device_isnt_a_virtiofsd() {
        let root = crate::testing::temp_dir("virtiofsd-gpu");
        let binary = root.join("vhost-user-gpu");
        std::fs::write(&binary, "").unwrap();
        std::fs::write(root.join("50-qemu-gpu.json"), descriptor("gpu", &binary)).unwrap();
        std::fs::write(root.join("README"), descriptor("fs", &binary)).unwrap();
        assert_eq!(found_in(&[&root]), None);
    }

    #[test]
    fn a_shared_folder_on_libvirt_without_virtiofsd_is_refused_with_its_code_before_anything_starts() {
        use crate::error::Code;
        let refused = share_on(Provider::Libvirt, Os::Linux, || false).unwrap_err();
        assert_eq!(refused.code(), Code::VirtiofsdMissing);
        assert!(refused.to_string().contains("apt-get install virtiofsd"), "says what to install: {refused}");
        assert!(share_on(Provider::Libvirt, Os::Linux, || true).is_ok());
        // Another hypervisor never asks for virtiofsd.
        assert!(share_on(Provider::Virtualbox, Os::Linux, || panic!("not asked")).is_ok());
    }

    #[test]
    fn only_libvirt_on_linux_is_held_to_a_virtiofsd() {
        assert!(needs_virtiofsd(Provider::Libvirt, Os::Linux));
        for (p, os) in [(Provider::Qemu, Os::Linux), (Provider::Virtualbox, Os::Linux), (Provider::Libvirt, Os::Macos)] {
            assert!(!needs_virtiofsd(p, os), "{p:?} on {os:?}");
        }
    }
}
