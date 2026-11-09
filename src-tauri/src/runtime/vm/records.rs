//! What Vagrant still records of a VM lab's machines in its folder.

use std::path::Path;

/// Whether Vagrant still tracks a created machine of this folder (on any provider): it writes
/// `.vagrant/machines/<name>/<provider>/id` when the machine is created and removes it when it
/// is destroyed. Read from the files only, so it never reaches the host.
pub(crate) fn has_machines(dir: &Path) -> bool {
    let Ok(machines) = std::fs::read_dir(dir.join(".vagrant").join("machines")) else { return false };
    machines.flatten().any(|m| std::fs::read_dir(m.path()).is_ok_and(|provs| provs.flatten().any(|p| p.path().join("id").is_file())))
}
