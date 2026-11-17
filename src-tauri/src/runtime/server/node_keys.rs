//! The node host keys a lab run on a saved Proxmox server is checked against. When its lab bridge
//! is internal to the node, every SSH of the run goes through the node first, and checks it
//! strictly against the keys the player confirmed (`nodes_known_hosts`): never on first use. A
//! server set up before its node's keys were confirmed has none there, so before such a run (and
//! at Test connection) the player is shown the keys the node offers, as the server setup shows
//! them (`setup::identity`), and confirms them once. Keys other than the confirmed ones are
//! refused, as every SSH to the node refuses them.

use std::path::{Path, PathBuf};

use super::contract::proxmox_jump_node;
use crate::error::{Code, Error, Result};
use crate::platform::AppDirs;
use crate::runtime::ssh::known_hosts::{self, Offered};

/// The port the run reaches the node on (Isothrum's `ssh_jump` names none: SSH's own).
const JUMP_PORT: u16 = 22;

/// The keys saved host `id`'s node offers when the player has to confirm them before a run: none
/// when its labs aren't reached through the node, or its keys are the confirmed ones. Others than
/// the confirmed ones are refused (`SshHostKeyRefused`).
pub(crate) async fn unconfirmed(dirs: &AppDirs, id: &str) -> Result<Option<Offered>> {
    let Some(node) = proxmox_jump_node(dirs, id).await? else { return Ok(None) };
    unconfirmed_at(&node, &confirmed_file(dirs).await?).await
}

/// Confirms the keys saved host `id`'s node offers, which the player was shown as `fingerprints`
/// ([`unconfirmed`]). Never over keys confirmed before: those are refused, not replaced (the
/// server's setup is where a reinstalled node's new keys are confirmed).
pub(crate) async fn confirm(dirs: &AppDirs, id: &str, fingerprints: &[String]) -> Result<()> {
    let Some(node) = proxmox_jump_node(dirs, id).await? else {
        return Err(Error::Invalid("this server's labs aren't reached through its node: it has no node keys to confirm".into()));
    };
    confirm_at(&node, fingerprints, &confirmed_file(dirs).await?).await
}

/// The node keys the player confirmed (`nodes_known_hosts`, beside the launcher's key).
async fn confirmed_file(dirs: &AppDirs) -> Result<PathBuf> {
    let (launcher, _) = crate::runtime::ssh::ensure_key(dirs).await?;
    Ok(crate::runtime::ssh::node_known_hosts(&launcher))
}

/// [`unconfirmed`] for `node`, against the keys confirmed in `file`.
pub(crate) async fn unconfirmed_at(node: &str, file: &Path) -> Result<Option<Offered>> {
    let offered = known_hosts::offered(node, JUMP_PORT, file).await?;
    match offered.state {
        known_hosts::State::Confirmed => Ok(None),
        known_hosts::State::New => Ok(Some(offered)),
        known_hosts::State::Changed => Err(Error::coded(Code::SshHostKeyRefused { host: node.to_owned() })),
    }
}

/// [`confirm`] for `node`, into `file`: nothing to do when they are confirmed already.
pub(crate) async fn confirm_at(node: &str, fingerprints: &[String], file: &Path) -> Result<()> {
    if unconfirmed_at(node, file).await?.is_none() {
        return Ok(());
    }
    known_hosts::confirm(node, JUMP_PORT, fingerprints, &[file.to_path_buf()]).await
}

#[cfg(test)]
mod tests;
