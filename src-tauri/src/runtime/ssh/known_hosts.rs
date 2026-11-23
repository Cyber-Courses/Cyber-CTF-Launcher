//! The SSH host keys of the server nodes the launcher signs in to (a Proxmox node: its key setup,
//! the snippet upload, the jump to an internal lab bridge), trusted on first use. Reading the keys
//! a node offers and writing the confirmed ones are isothrum-machine's
//! (`place::ssh::known_hosts`); the learner confirms them in Servers, and every SSH to the node
//! checks them strictly. A node offering other keys is refused until the learner confirms the new
//! ones.

use std::path::{Path, PathBuf};

use isothrum_machine::place::ssh::known_hosts as machine;
#[cfg(test)]
pub use isothrum_machine::place::ssh::known_hosts::Shown;
pub use isothrum_machine::place::ssh::known_hosts::{Offered, State};

use crate::error::{Error, Result};

/// The keys `host` offers on `port`, for the learner to confirm, and whether they are the ones
/// confirmed in `file` already.
pub async fn offered(host: &str, port: u16, file: &Path) -> Result<Offered> {
    machine::offered(&super::machine(), host, port, file).await.map_err(|e| said(host, &e))
}

/// Confirms the keys `host` offers on `port`, which the learner was shown as `fingerprints`: read
/// again, they must be exactly those (a node that changed its keys meanwhile is refused), then
/// they replace any keys `files` had for it.
pub async fn confirm(host: &str, port: u16, fingerprints: &[String], files: &[PathBuf]) -> Result<()> {
    machine::confirm(&super::machine(), host, port, fingerprints, files).await.map_err(|e| said(host, &e))
}

/// A failed read or confirmation, as the learner reads it.
fn said(host: &str, e: &super::SshError) -> Error {
    let why = e.to_string();
    if why.contains("changed while they were being confirmed") {
        Error::Invalid(format!("the SSH host keys of {host} changed while you were confirming them: check them again"))
    } else if why.contains("offered no SSH host key") || why.contains("unexpected SSH") {
        Error::Invalid(why)
    } else {
        Error::Invalid(format!("couldn't read the SSH host keys of {host} ({why})"))
    }
}

#[cfg(test)]
mod tests;
