//! SSH to the Proxmox node, isothrum-machine's (`place::ssh`, `place::proxmox::node`): its login,
//! letting the launcher's key in so labs on an internal bridge can be reached through it, and what
//! the node's own system says. The launcher keeps where the key lives and what the player reads.

use std::path::Path;

use isothrum_machine::place::proxmox::{bootstrap, node};
use isothrum_machine::place::ssh::{SshError, Target};

use super::{HostProfile, ssh, token_user};

/// The node's SSH login (`user@host`) for going through it.
pub fn node_login(h: &HostProfile) -> String {
    format!("{}@{}", ssh_user(&h.username), h.host)
}

/// The node of host `h` signed in to with the launcher's key `identity`, its host keys the ones
/// confirmed beside that key.
pub(super) fn by_key(h: &HostProfile, identity: &Path) -> Target {
    Target::new(h.host.clone(), ssh_user(&h.username), identity.to_path_buf(), ssh::node_known_hosts(identity))
}

/// The same, signed in with `password` (answered through an askpass script beside the key, in the
/// player's own app data: never the shared system temp dir, where a file another user left under
/// that name couldn't be rewritten, or could be swapped for something else).
pub(super) fn by_password(h: &HostProfile, password: &str, identity: &Path) -> Target {
    let folder = identity.parent().map_or_else(|| identity.to_path_buf(), Path::to_path_buf);
    by_key(h, identity).with_password(password, folder)
}

/// Lets the launcher's key into the node's SSH (password hosts; a token setup already needs
/// it, see `ssh_key_problem`), so labs on an internal bridge can be reached through the node.
/// Uses the stored password once, through SSH's own askpass hook; idempotent.
pub async fn authorize_launcher_key(h: &HostProfile, password: &str, identity: &Path, public: &str) -> crate::error::Result<()> {
    bootstrap::authorize(&ssh::machine(), &by_password(h, password, identity), public).await.map(drop).map_err(|e| {
        if e.host_key_refused { ssh::ssh_error(&h.host, SshError::HostKeyRefused { host: h.host.clone() }) } else { crate::error::Error::Invalid(e.detail) }
    })
}

/// What's wrong, and Proxmox's documented fix, when the node doesn't source its SDN config.
pub(super) const SDN_MISSING: &str = "The node's /etc/network/interfaces doesn't load the SDN configuration, so a lab's network (an SDN vnet bridge) is never created and its VMs fail to start with \"bridge ... does not exist\". On the node, add this line at the end of /etc/network/interfaces:\n  source /etc/network/interfaces.d/*\nthen run: ifreload -a";

/// The problem a launch would hit when the node doesn't source its SDN config (checked over SSH:
/// the launcher's key `identity` on token setups, the password otherwise). A token setup whose SSH
/// fails is reported by `ssh_key_problem` already.
pub(super) async fn sdn_problem(h: &HostProfile, password: &str, identity: &Path) -> Option<String> {
    let token = super::is_token(&h.username);
    let target = if token { by_key(h, identity) } else { by_password(h, password, identity) };
    sdn_outcome(node::sdn_sourced(&ssh::machine(), &target).await, (!token).then(|| node_login(h)))
}

/// `sdn_problem` from what the check over SSH gave: no directive, or SSH itself failed, reported
/// for a password setup (`password_login`, the login it tried) and left to `ssh_key_problem` for a
/// token one.
pub(super) fn sdn_outcome(checked: Result<bool, SshError>, password_login: Option<String>) -> Option<String> {
    match (checked, password_login) {
        (Ok(false), _) => Some(SDN_MISSING.into()),
        (Err(e), Some(login)) => Some(format!("Couldn't check the node's network configuration over SSH as {login} ({}).", ssh::ssh_error(&login, e))),
        (Ok(true), _) | (Err(_), None) => None,
    }
}

/// The SSH user for the snippet upload: the token's user without its realm.
pub fn ssh_user(username: &str) -> &str {
    token_user(username).split('@').next().unwrap_or("root")
}

/// Token setups upload the snippet over SSH with the launcher's key `identity`: checks it's
/// authorized.
pub(super) async fn ssh_key_problem(h: &HostProfile, identity: &Path) -> Option<String> {
    let user = ssh_user(&h.username).to_string();
    if node::signs_in(&ssh::machine(), &by_key(h, identity)).await.is_ok() {
        return None;
    }
    let public = identity.with_extension("pub");
    Some(key_not_authorized(&user, &h.host, &public, &std::fs::read_to_string(&public)))
}

/// What to do when the node refuses the launcher's key: add it, the command to run on the node
/// written out with the public key read from `public`. A public key that can't be read (or is
/// empty) is said instead: the command would add an empty line.
fn key_not_authorized(user: &str, host: &str, public: &std::path::Path, read: &std::io::Result<String>) -> String {
    let lead = format!("The launcher can't SSH to {user}@{host} with its key, which a token setup needs to upload the lab's cloud-init.");
    match read.as_ref().map(|k| k.trim()) {
        Ok(key) if !key.is_empty() => format!("{lead} On the node, run:\n  echo '{key}' >> ~/.ssh/authorized_keys"),
        Ok(_) => format!("{lead} Its public key ({}) is empty: set the server up again in the launcher to make a new key.", public.display()),
        Err(e) => format!("{lead} Its public key ({}) can't be read ({e}): set the server up again in the launcher to make a new key.", public.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_key_says_the_command_with_the_key_or_why_there_is_none() {
        let path = std::path::Path::new("/keys/id_ed25519.pub");
        let ok = key_not_authorized("root", "pve", path, &Ok("ssh-ed25519 AAAA launcher\n".into()));
        assert!(ok.ends_with("echo 'ssh-ed25519 AAAA launcher' >> ~/.ssh/authorized_keys"), "{ok}");
        // Regression: a public key that couldn't be read gave `echo '' >> ~/.ssh/authorized_keys`.
        for read in [Ok("  \n".to_string()), Err(std::io::Error::from(std::io::ErrorKind::NotFound))] {
            let said = key_not_authorized("root", "pve", path, &read);
            assert!(!said.contains("echo ''") && said.contains("/keys/id_ed25519.pub") && said.contains("set the server up again"), "{said}");
        }
    }
}
