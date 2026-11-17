//! The launcher's own SSH key and the server nodes it reaches with it. SSH itself is
//! isothrum-machine's (`isothrum_machine::place::ssh`): the key is made there, at the path the
//! launcher has always kept it (`<app data>/ssh/id_ed25519`), so nodes already set up keep letting
//! it in; the launcher keeps where the key and the nodes' confirmed host keys live.
//!
//! Vagrant targets (ESXi) use Vagrant's own key, read from `vagrant ssh-config`.

use std::path::{Path, PathBuf};

pub use isothrum_machine::place::ssh::SshError;

use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// The `known_hosts` every attack box shell once shared. Its keys of lab hosts long gone refused
/// the shell to any new lab host at a recycled address: removed (nothing writes it any more).
const SHARED_KNOWN_HOSTS: &str = "known_hosts";

/// The comment the launcher's key carries.
const KEY_COMMENT: &str = "cyberctf-launcher";

pub mod known_hosts;

fn ssh_dir(dirs: &AppDirs) -> Result<PathBuf> {
    let dir = dirs.data()?.join("ssh");
    std::fs::create_dir_all(&dir)?;
    let _ = std::fs::remove_file(dir.join(SHARED_KNOWN_HOSTS));
    Ok(dir)
}

/// What SSH runs with: isothrum-machine's context for this machine (a test's scripted tools in a
/// test).
pub(crate) fn machine() -> isothrum_machine::Context {
    crate::runtime::providers::machine()
}

/// The launcher's private key, made on first use. Returns (key path, public key line).
pub async fn ensure_key(dirs: &AppDirs) -> Result<(PathBuf, String)> {
    let key = ssh_dir(dirs)?.join("id_ed25519");
    let public = isothrum_machine::place::ssh::ensure_key(&machine(), &key, KEY_COMMENT).await.map_err(|e| ssh_error("", e))?;
    Ok((key, public))
}

/// The launcher's error for a failed SSH to `host`: a host key that isn't the confirmed one is the
/// learner's error naming the node; anything else as it is.
pub fn ssh_error(host: &str, e: SshError) -> Error {
    match e {
        SshError::HostKeyRefused { .. } => Error::coded(crate::error::Code::SshHostKeyRefused { host: host.to_owned() }),
        other => Error::Invalid(other.to_string()),
    }
}

/// The host keys of the server nodes the launcher reaches with its key (a Proxmox node: the jump
/// host to an internal lab bridge, its key setup), next to that key, as the learner confirmed
/// them ([`known_hosts`]). A lab host's keys are never kept here: the Isothrum runner keeps each
/// run's own, in the run's folder.
pub fn node_known_hosts(identity: &Path) -> PathBuf {
    identity.with_file_name("nodes_known_hosts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_known_hosts_is_dropped_and_nodes_keep_their_own() {
        let t = crate::testing::test_app::TestApp::new();
        let shared = t.data().join("ssh").join(SHARED_KNOWN_HOSTS);
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, "10.0.0.5 ssh-ed25519 AAAAold\n").unwrap();
        let dir = ssh_dir(&t.dirs()).unwrap();
        assert!(!shared.exists());
        assert_eq!(node_known_hosts(&dir.join("id_ed25519")), dir.join("nodes_known_hosts"));
    }
}
