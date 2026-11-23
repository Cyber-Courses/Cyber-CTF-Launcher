//! A Proxmox VE host for the Server screen: its API is isothrum-machine's
//! (`isothrum_machine::place::proxmox`, signed in by an API token or a user's password, its
//! self-signed certificate pinned); the launcher keeps the saved host's settings and what the
//! player is told.
//!
//! A token is recognised by its id, `user@realm!name` (the username field holds it and the
//! keychain holds its secret). The cloud-init snippet still goes over SSH, which a token can't do:
//! token setups SSH as the token's user with the launcher's key.

#[cfg(test)]
use isothrum_machine::place::proxmox::api;
use isothrum_machine::place::proxmox::check::{DEFAULT_BRIDGE, DEFAULT_DIR_STORAGE, DEFAULT_DISK_STORAGE, Needs};
use isothrum_machine::place::proxmox::{self as machine, Credentials, Server};
pub use isothrum_machine::place::proxmox::{CallError, is_token, terraform_token, token_user, with_default_realm};

use super::server::HostProfile;
use super::server::tls_pin::{self, Refusal};
use super::ssh;

mod check;
mod node_ssh;
pub mod placement;
pub mod ssh_key;

pub use check::{bridge_is_internal, test};
#[cfg(test)]
use check::{node_report, outcome, sign_in_report, storage_problems};
#[cfg(test)]
use node_ssh::{SDN_MISSING, sdn_outcome};
pub use node_ssh::{authorize_launcher_key, node_login, ssh_user};
use node_ssh::{sdn_problem, ssh_key_problem};

/// The storages Isoloom's Proxmox module puts the cloud image and the cloud-init snippets on
/// when the host names none (the module's defaults).
pub const IMAGE_STORAGE: &str = DEFAULT_DIR_STORAGE;
/// The storage the cloud-init snippets go on when the host names none (the module's default).
pub const SNIPPET_STORAGE: &str = DEFAULT_DIR_STORAGE;

/// The host's storage for cloud images, else the module's default.
pub fn image_storage(h: &HostProfile) -> String {
    h.image_storage.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| IMAGE_STORAGE.into())
}

/// The host's storage for cloud-init snippets, else the module's default.
pub fn snippet_storage(h: &HostProfile) -> String {
    h.snippet_storage.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| SNIPPET_STORAGE.into())
}

/// What a lab on host `h` needs of its node: the storages and the bridge it names, else the
/// module's defaults.
pub(crate) fn needs(h: &HostProfile) -> Needs {
    Needs {
        disk_storage: h.datastore.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_DISK_STORAGE.into()),
        image_storage: image_storage(h),
        snippet_storage: snippet_storage(h),
        bridge: lab_bridge(h),
    }
}

/// A Proxmox API token id rather than a user, as a pattern: `user@realm!name`, a realm before
/// the first `!` and a name after it. The setup form tells a token from a user by it as it is
/// typed (the generated `Patterns.ts`), so it is written in the syntax both regex engines read;
/// a test holds it to [`is_token`]. Only the generated TypeScript reads it (`testing::ts_bindings`).
#[cfg(test)]
pub const TOKEN_PATTERN: &str = r"^[^!]*@[^!]*![\s\S]+$";

/// The server `h` is, as its API is reached.
fn server(h: &HostProfile) -> Server {
    Server::new(h.host.clone(), h.port)
}

/// The API base URL, e.g. `https://pve.lan:8006/api2/json`.
pub fn api_url(h: &HostProfile) -> String {
    server(h).api_url()
}

/// The endpoint the Terraform provider takes, e.g. `https://pve.lan:8006/`.
pub fn terraform_endpoint(h: &HostProfile) -> String {
    server(h).endpoint()
}

/// The bridge lab VMs attach to (the host's setting, else the Proxmox default).
fn lab_bridge(h: &HostProfile) -> String {
    h.network.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_BRIDGE.into())
}

/// A signed-in API client (isothrum-machine's), its calls' failures worded for the player.
pub struct Session(machine::Session);

impl Session {
    /// The signed-in client itself.
    pub(crate) fn api(&self) -> &machine::Session {
        &self.0
    }

    /// The configured node of `h`, or the first one when none is set.
    pub async fn node(&self, h: &HostProfile) -> Result<String, CallError> {
        self.0.node(h.node.as_deref().filter(|n| !n.is_empty())).await.map_err(worded)
    }
}

/// Why signing in to a Proxmox host failed.
#[derive(Debug)]
pub enum SignInError {
    /// The host answered and refused the credentials.
    Rejected,
    /// Its self-signed certificate isn't the one the learner confirmed (or none was yet): nothing
    /// was sent to it. The error says it with the host's name ([`SignInError::into_error`]).
    Certificate(Refusal, String),
    /// Couldn't talk to the API (TLS, network, unexpected answer).
    Failed(String),
}

impl std::fmt::Display for SignInError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignInError::Rejected => f.write_str("Proxmox rejected the credentials"),
            SignInError::Certificate(refusal, server) => write!(f, "{}", tls_pin::refused(*refusal, server)),
            SignInError::Failed(e) => f.write_str(e),
        }
    }
}

impl SignInError {
    /// The launcher's error for it: a refused certificate keeps its code (the learner confirms it
    /// in Servers), anything else is said after `context`.
    pub fn into_error(self, context: &str) -> crate::error::Error {
        match self {
            SignInError::Certificate(refusal, server) => tls_pin::refused(refusal, &server),
            other => crate::error::Error::Invalid(format!("{context}: {other}")),
        }
    }
}

/// Signs in to the Proxmox API of host `h` with `secret` (its API token's secret, or its user's
/// password for a ticket).
///
/// A self-signed certificate is accepted only when it is the one the learner confirmed for the host
/// ([`tls_pin`]): any other is refused in the handshake, before the token or password is sent.
pub async fn sign_in(h: &HostProfile, secret: &str) -> Result<Session, SignInError> {
    machine::sign_in(&server(h), &Credentials::from_login(&h.username, secret), h.tls_trust()).await.map(Session).map_err(|e| match e {
        machine::SignInError::Rejected => SignInError::Rejected,
        machine::SignInError::Certificate(refusal) => SignInError::Certificate(refusal, h.name.clone()),
        machine::SignInError::Failed(why) if why.to_lowercase().contains("certificate") => SignInError::Failed(
            "the host's TLS certificate isn't trusted. Proxmox uses a self-signed certificate by default: turn on \"Self-signed certificate\" for this host and confirm its fingerprint, or install a trusted one".into(),
        ),
        other => SignInError::Failed(other.to_string()),
    })
}

/// A failed call worded for the player (`CallError::Other` holding the sentence when isothrum-machine's
/// own differs).
fn worded(e: CallError) -> CallError {
    match e {
        e @ (CallError::NoSuchNode { .. } | CallError::Certificate(_)) => CallError::Other(said(&e)),
        e => e,
    }
}

/// A failed API call, as the player reads it.
pub(crate) fn said(e: &CallError) -> String {
    match e {
        CallError::NoSuchNode { asked, found } => format!("there is no node \"{asked}\" on this host (found: {})", found.join(", ")),
        CallError::Certificate(refusal) => tls_pin::refused(*refusal, "the Proxmox server").to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
