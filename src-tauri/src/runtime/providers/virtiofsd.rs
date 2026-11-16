//! Whether a hypervisor finds the virtiofsd it shares a folder of this computer with a VM through
//! (the attack box's shared folder), on Linux:
//! - libvirt: vagrant-libvirt shares the folder as a `virtiofs` filesystem, and libvirt starts the
//!   `virtiofsd` a vhost-user descriptor names for it. With none, the VM's start fails deep in
//!   vagrant-libvirt ("Unable to find a satisfying virtiofsd").
//! - QEMU: vagrant-qemu (0.7.0 on) starts the `virtiofsd` it finds itself, as its `virtiofsd_bin`
//!   defaults (which no lab sets): on the `PATH`, then `/usr/libexec/virtiofsd` (the package's,
//!   Debian, Ubuntu, Fedora), then `/usr/lib/qemu/virtiofsd` (QEMU's own, before the package).
//!   With none, Vagrant stops with its error ("virtiofsd was not found or is not executable").
//!
//! Without one, the hypervisor isn't ready. virtiofsd is its own package from Debian 12, Ubuntu
//! 23.10 and Fedora 36 on (QEMU's before), and neither libvirt nor QEMU depends on it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::Provider;
use crate::platform::host::Os;

/// Where libvirt reads QEMU's vhost-user descriptors, in the order it reads them: a descriptor in
/// a later folder takes the place of an earlier one of the same name (an administrator's, in
/// `/etc`, over the distribution's). libvirt's own system daemon (`qemu:///system`, which Vagrant
/// uses) reads no player's folder.
const VHOST_USER_DIRS: [&str; 2] = ["/usr/share/qemu/vhost-user", "/etc/qemu/vhost-user"];

/// Where vagrant-qemu looks for a virtiofsd after the `PATH`, in its order (its
/// `Config#find_virtiofsd`).
const VAGRANT_QEMU_FALLBACKS: [&str; 2] = ["/usr/libexec/virtiofsd", "/usr/lib/qemu/virtiofsd"];

/// Whether `provider`'s VMs need a virtiofsd on `os` for the folders this computer shares with
/// them: libvirt's and QEMU's, on Linux (QEMU on macOS shares none, `ProviderRow::shares_folders`).
pub(crate) fn needs_virtiofsd(provider: Provider, os: Os) -> bool {
    matches!(provider, Provider::Libvirt | Provider::Qemu) && os == Os::Linux
}

/// Whether `provider` finds a virtiofsd on this machine, where it looks: libvirt [`found_in`] its
/// descriptor folders, vagrant-qemu [`vagrant_qemus`] on the `PATH` (this process's, which Vagrant
/// inherits) and its fallbacks. Always off Linux, where neither needs one, and in a test whose
/// hypervisors are fakes.
pub(crate) fn found(provider: Provider) -> bool {
    !cfg!(target_os = "linux")
        || super::kvm::devices_assumed_ready()
        || match provider {
            Provider::Qemu => vagrant_qemus(std::env::var_os("PATH").as_deref(), &VAGRANT_QEMU_FALLBACKS).is_some(),
            _ => found_in(&VHOST_USER_DIRS).is_some(),
        }
}

/// The virtiofsd vagrant-qemu would start: the first `virtiofsd` on `path`, then of `fallbacks`,
/// that is a file this account may run, as vagrant-qemu looks (an empty `PATH` entry skipped).
fn vagrant_qemus(path: Option<&OsStr>, fallbacks: &[impl AsRef<Path>]) -> Option<PathBuf> {
    let on_path = path.into_iter().flat_map(std::env::split_paths).filter(|d| !d.as_os_str().is_empty()).map(|d| d.join("virtiofsd"));
    on_path.chain(fallbacks.iter().map(|f| f.as_ref().to_path_buf())).find(|f| runnable(f))
}

/// Whether `file` is a file this account may run (an execute bit set, on Unix).
fn runnable(file: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        file.is_file()
    }
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
/// or QEMU without a virtiofsd ([`found`]), whose start would otherwise fail in its Vagrant plugin.
pub(crate) fn can_share(provider: Provider) -> crate::error::Result<()> {
    share_on(provider, Os::HERE, found)
}

/// [`can_share`] on `os`, `found` telling whether `provider` finds a virtiofsd (asked only of the
/// providers that need one).
fn share_on(provider: Provider, os: Os, found: impl FnOnce(Provider) -> bool) -> crate::error::Result<()> {
    if needs_virtiofsd(provider, os) && !found(provider) {
        return Err(crate::error::Error::coded(crate::error::Code::VirtiofsdMissing { hypervisor: provider.label().to_owned() }));
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

    #[cfg(unix)]
    #[test]
    fn qemu_finds_the_virtiofsd_vagrant_qemu_starts_where_it_looks() {
        // vagrant-qemu's own lookup (`Config#find_virtiofsd`): the PATH, then /usr/libexec (the
        // package's), then /usr/lib/qemu (QEMU's own, before the package). Without one, the lab's
        // start stopped at "virtiofsd was not found or is not executable".
        let root = crate::testing::temp_dir("virtiofsd-qemu");
        let (bin, libexec, qemu) = (root.join("bin"), root.join("libexec"), root.join("qemu"));
        for d in [&bin, &libexec, &qemu] {
            std::fs::create_dir_all(d).unwrap();
        }
        let path = std::env::join_paths([&bin]).unwrap();
        let fallbacks = [libexec.join("virtiofsd"), qemu.join("virtiofsd")];
        assert_eq!(vagrant_qemus(Some(&path), &fallbacks), None, "none anywhere");

        // A file it may not run isn't one, as vagrant-qemu reads it (`File.executable?`).
        std::fs::write(&fallbacks[1], "").unwrap();
        assert_eq!(vagrant_qemus(Some(&path), &fallbacks), None, "not executable");
        crate::testing::write_executable(&fallbacks[1], "#!/bin/sh\n");
        assert_eq!(vagrant_qemus(Some(&path), &fallbacks), Some(fallbacks[1].clone()), "QEMU's own");
        crate::testing::write_executable(&fallbacks[0], "#!/bin/sh\n");
        assert_eq!(vagrant_qemus(Some(&path), &fallbacks), Some(fallbacks[0].clone()), "the package's first");
        crate::testing::write_executable(bin.join("virtiofsd"), "#!/bin/sh\n");
        assert_eq!(vagrant_qemus(Some(&path), &fallbacks), Some(bin.join("virtiofsd")), "the PATH's first");
        // A folder named virtiofsd isn't one; an empty PATH entry isn't the current folder.
        std::fs::remove_file(bin.join("virtiofsd")).unwrap();
        std::fs::create_dir(bin.join("virtiofsd")).unwrap();
        let path = std::env::join_paths([Path::new(""), &bin]).unwrap();
        assert_eq!(vagrant_qemus(Some(&path), &fallbacks), Some(fallbacks[0].clone()));
        // And libvirt's descriptors play no part for QEMU.
        assert_eq!(vagrant_qemus(None, &[] as &[PathBuf]), None);
    }

    #[test]
    fn a_shared_folder_on_libvirt_or_qemu_without_virtiofsd_is_refused_with_its_code_before_anything_starts() {
        use crate::error::Code;
        for (provider, name) in [(Provider::Libvirt, "libvirt"), (Provider::Qemu, "QEMU")] {
            let refused = share_on(provider, Os::Linux, |p| {
                assert_eq!(p, provider, "asked of the lab's own hypervisor");
                false
            })
            .unwrap_err();
            assert_eq!(refused.code(), Code::VirtiofsdMissing { hypervisor: name.into() });
            let said = refused.to_string();
            assert!(said.starts_with(&format!("{name} can't share")) && said.contains("apt-get install virtiofsd"), "says what to install: {said}");
            assert!(share_on(provider, Os::Linux, |_| true).is_ok());
        }
        // Another hypervisor, or QEMU on macOS, never asks for virtiofsd.
        assert!(share_on(Provider::Virtualbox, Os::Linux, |_| panic!("not asked")).is_ok());
        assert!(share_on(Provider::Qemu, Os::Macos, |_| panic!("not asked")).is_ok());
    }

    #[test]
    fn libvirt_and_qemu_on_linux_are_held_to_a_virtiofsd() {
        assert!(needs_virtiofsd(Provider::Libvirt, Os::Linux) && needs_virtiofsd(Provider::Qemu, Os::Linux));
        for (p, os) in [(Provider::Qemu, Os::Macos), (Provider::Virtualbox, Os::Linux), (Provider::Libvirt, Os::Macos)] {
            assert!(!needs_virtiofsd(p, os), "{p:?} on {os:?}");
        }
    }
}
