//! The launcher's own SSH key and the server nodes it reaches with it. SSH itself is
//! isothrum-machine's (`isothrum_machine::place::ssh`): the key is made there, at the path the
//! launcher has always kept it (`<app data>/ssh/id_ed25519`), so nodes already set up keep letting
//! it in; the launcher keeps where the key and the nodes' confirmed host keys live.
//!
//! Vagrant targets (ESXi) use Vagrant's own key, read from `vagrant ssh-config`.

use std::path::{Path, PathBuf};

pub use isothrum_machine::place::ssh::SshError;
#[cfg(test)]
pub use isothrum_machine::place::ssh::config_path as ssh_config_path;

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

/// A node reached with the launcher's SSH key, for the tunnel through it.
#[derive(Debug, PartialEq)]
pub struct Target {
    /// The address to connect to.
    pub host: String,
    /// Its SSH port.
    pub port: u16,
    /// The user to sign in as.
    pub user: String,
    /// The private key to sign in with.
    pub identity: PathBuf,
}

impl Target {
    /// The node login `login` (`user@host`, as a jump names it) on port 22, signed in with
    /// `identity`; refused when it isn't one.
    pub fn login(login: &str, identity: PathBuf) -> Result<Self> {
        let (user, host) = login.split_once('@').ok_or_else(|| Error::Invalid("unexpected jump host".into()))?;
        let target = Target { host: host.into(), port: 22, user: user.into(), identity };
        // The exec arguments refuse a host or user that isn't a plain word.
        target.node(Path::new("")).exec_args("true").map_err(|_| Error::Invalid("unexpected SSH host or user".into()))?;
        Ok(target)
    }

    /// The same as isothrum-machine's target, its host keys checked against `known_hosts`.
    fn node(&self, known_hosts: &Path) -> isothrum_machine::place::ssh::Target {
        isothrum_machine::place::ssh::Target::new(self.host.clone(), self.user.clone(), self.identity.clone(), known_hosts.to_path_buf()).on_port(self.port)
    }

    /// The `ssh` arguments of a SOCKS5 proxy on `127.0.0.1:port` through this host, signing in
    /// to nothing else (`-N`): unattended, its host key checked against `known_hosts`, and ending
    /// when the port can't be forwarded (taken meanwhile) rather than running on as no proxy.
    pub fn socks_args(&self, known_hosts: &Path, port: u16) -> Result<Vec<String>> {
        let mut args: Vec<String> = vec!["-N".into(), "-D".into(), format!("127.0.0.1:{port}")];
        let mut exec = self.node(known_hosts).exec_args("").map_err(|e| Error::Invalid(e.to_string()))?;
        // isothrum-machine's arguments end with `--`, the login and the command: the proxy runs none.
        exec.truncate(exec.len().saturating_sub(3));
        args.extend(exec);
        args.extend(["-o".into(), "ExitOnForwardFailure=yes".into(), format!("{}@{}", self.user, self.host)]);
        Ok(args)
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
    fn a_node_login_is_a_plain_user_and_host() {
        assert!(Target::login("root@pve.lan", "/k/id".into()).is_ok());
        for bad in ["nobody", "root@pve;rm", "-o@x", "root@-oProxyCommand=x"] {
            assert!(Target::login(bad, "/k/id".into()).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_socks_proxy_signs_in_to_nothing_with_the_key_and_the_confirmed_host_keys() {
        let node = Target::login("root@pve.lan", "/k/id".into()).unwrap();
        let args = node.socks_args(Path::new("/Users/a/Application Support/kh"), 1080).unwrap();
        assert_eq!(args[..5], ["-N", "-D", "127.0.0.1:1080", "-i", "/k/id"]);
        assert!(args.contains(&"BatchMode=yes".to_owned()) && args.contains(&"StrictHostKeyChecking=yes".to_owned()));
        assert!(args.contains(&r"UserKnownHostsFile=/Users/a/Application\ Support/kh".to_owned()));
        assert_eq!(args[args.len() - 3..], ["-o", "ExitOnForwardFailure=yes", "root@pve.lan"]);
    }

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
