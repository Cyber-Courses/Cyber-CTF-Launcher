//! Talking to a Proxmox VE host's API for the Server screen: sign in (API token or
//! user + password), then check what a lab launch will need, so problems show up in
//! "Test connection" with their fix instead of halfway through a launch.
//!
//! A token is recognised by its id, `user@realm!name` (the username field holds it and
//! the keychain holds its secret). The cloud-init snippet still goes over SSH, which a
//! token can't do: token setups SSH as the token's user with the launcher's key.

use std::time::Duration;

use serde::de::DeserializeOwned;

use self::api::{Iface, Node, Ticket};
use super::server::HostProfile;
use super::server::tls_pin::{self, Refusal};
use super::ssh;

pub mod api;
mod check;
mod node_ssh;
pub mod placement;
pub mod ssh_key;

pub use check::{bridge_is_internal, test};
#[cfg(test)]
use check::{bridge_problem, node_report, outcome, sign_in_report, storage_content, storage_problems};
#[cfg(test)]
use node_ssh::{SDN_MISSING, SDN_SOURCE_CHECK, askpass_script, authorize_args, sdn_outcome, write_askpass};
pub use node_ssh::{authorize_launcher_key, node_login, ssh_user};
use node_ssh::{sdn_problem, ssh_key_problem};

const TIMEOUT: Duration = Duration::from_secs(8);

/// The storages Isoloom's Proxmox module puts the cloud image and the cloud-init snippets on
/// when the host names none (the module's defaults).
pub const IMAGE_STORAGE: &str = "local";
/// The storage the cloud-init snippets go on when the host names none (the module's default).
pub const SNIPPET_STORAGE: &str = "local";

/// The host's storage for cloud images, else the module's default.
pub fn image_storage(h: &HostProfile) -> String {
    h.image_storage.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| IMAGE_STORAGE.into())
}

/// The host's storage for cloud-init snippets, else the module's default.
pub fn snippet_storage(h: &HostProfile) -> String {
    h.snippet_storage.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| SNIPPET_STORAGE.into())
}

/// A Proxmox login with its realm: `root` is `root@pam`, a token `root!name` is `root@pam!name`;
/// one with a realm (`admin@pve`, `root@pam!ci`) stays as typed.
pub fn with_default_realm(username: &str) -> String {
    let (user, token) = username.split_once('!').map_or((username, None), |(u, t)| (u, Some(t)));
    let user = if user.is_empty() || user.contains('@') { user.to_string() } else { format!("{user}@pam") };
    token.map_or_else(|| user.clone(), |t| format!("{user}!{t}"))
}

/// A Proxmox API token id rather than a user, as a pattern: `user@realm!name`, a realm before
/// the first `!` and a name after it. The setup form tells a token from a user by it as it is
/// typed (the generated `Patterns.ts`), so it is written in the syntax both regex engines read.
pub const TOKEN_PATTERN: &str = r"^[^!]*@[^!]*![\s\S]+$";

/// [`TOKEN_PATTERN`], compiled; None never happens (a test compiles it), and would read every
/// username as a user.
static TOKEN: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| regex::Regex::new(TOKEN_PATTERN).ok());

/// Whether `username` is a Proxmox API token id ([`TOKEN_PATTERN`]) rather than a user.
pub fn is_token(username: &str) -> bool {
    TOKEN.as_ref().is_some_and(|re| re.is_match(username))
}

/// The user a token belongs to (`root@pam!cyberctf` -> `root@pam`).
pub fn token_user(username: &str) -> &str {
    username.split_once('!').map_or(username, |(u, _)| u)
}

/// The `api_token` value the Terraform provider takes.
pub fn terraform_token(username: &str, secret: &str) -> String {
    format!("{username}={secret}")
}

/// A host as it goes in a URL (IPv6 in brackets).
fn host_for_url(host: &str) -> String {
    if host.contains(':') { format!("[{host}]") } else { host.to_string() }
}

/// The API base URL, e.g. `https://pve.lan:8006/api2/json`.
pub fn api_url(h: &HostProfile) -> String {
    format!("https://{}:{}/api2/json", host_for_url(&h.host), h.port)
}

/// The endpoint the Terraform provider takes, e.g. `https://pve.lan:8006/`.
pub fn terraform_endpoint(h: &HostProfile) -> String {
    format!("https://{}:{}/", host_for_url(&h.host), h.port)
}

/// The bridge lab VMs attach to (the host's setting, else the Proxmox default).
fn lab_bridge(h: &HostProfile) -> String {
    h.network.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "vmbr0".into())
}

/// An interface of `/nodes/<node>/network` by name.
fn find_iface<'a>(ifaces: &'a [Iface], name: &str) -> Option<&'a Iface> {
    ifaces.iter().find(|i| i.name.as_deref() == Some(name))
}

/// A bridge without physical ports (`bridge_ports` empty or missing).
fn has_no_ports(iface: &Iface) -> bool {
    iface.bridge_ports.as_deref().map(str::trim).unwrap_or_default().is_empty()
}

/// A signed-in API client.
pub struct Session {
    client: reqwest::Client,
    base: String,
    /// The header that authenticates each call: a ticket cookie or the token.
    auth: (&'static str, String),
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
            SignInError::Certificate(refusal, server) => write!(f, "{}", refusal.error(server)),
            SignInError::Failed(e) => f.write_str(e),
        }
    }
}

impl SignInError {
    /// The launcher's error for it: a refused certificate keeps its code (the learner confirms it
    /// in Servers), anything else is said after `context`.
    pub fn into_error(self, context: &str) -> crate::error::Error {
        match self {
            SignInError::Certificate(refusal, server) => refusal.error(&server),
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
    let pinned = tls_pin::Client::new(h.tls_trust(), TIMEOUT).map_err(|e| SignInError::Failed(e.to_string()))?;
    let refused = |e: String| match pinned.refused() {
        Some(refusal) => SignInError::Certificate(refusal, h.name.clone()),
        None => SignInError::Failed(e),
    };
    let client = pinned.http.clone();
    let base = api_url(h);
    if is_token(&h.username) {
        let session = Session { client, base, auth: ("Authorization", format!("PVEAPIToken={}={secret}", h.username)) };
        // Any authenticated call proves the token.
        return match session.call::<serde::de::IgnoredAny>("/version").await {
            Ok(_) => Ok(session),
            Err(CallError::Status(401)) => Err(SignInError::Rejected),
            Err(e) => Err(refused(e.to_string())),
        };
    }
    let res = client
        .post(format!("{base}/access/ticket"))
        .form(&[("username", h.username.as_str()), ("password", secret)])
        .send()
        .await
        .map_err(|e| refused(describe(&e)))?;
    if res.status().as_u16() == 401 {
        return Err(SignInError::Rejected);
    }
    if !res.status().is_success() {
        return Err(SignInError::Failed(format!("the Proxmox API answered HTTP {}", res.status())));
    }
    let body = res.text().await.map_err(|e| SignInError::Failed(e.to_string()))?;
    let answer: Ticket = api::data(&body).map_err(|e| SignInError::Failed(e.to_string()))?;
    let ticket = answer.ticket.ok_or_else(|| SignInError::Failed("no ticket in the sign-in answer".into()))?;
    Ok(Session { client, base, auth: ("Cookie", format!("PVEAuthCookie={ticket}")) })
}

/// Turns a reqwest error into one readable line, calling out the usual self-signed case.
pub fn describe(e: &reqwest::Error) -> String {
    let detail = std::iter::successors(Some(e as &dyn std::error::Error), |e| e.source()).map(std::string::ToString::to_string).collect::<Vec<_>>().join(": ");
    if detail.to_lowercase().contains("certificate") {
        "the host's TLS certificate isn't trusted. Proxmox uses a self-signed certificate by default: turn on \"Self-signed certificate\" for this host and confirm its fingerprint, or install a trusted one".into()
    } else {
        detail
    }
}

/// Why a call to the Proxmox API failed.
#[derive(Debug)]
pub enum CallError {
    /// The API answered with this HTTP status, not a success.
    Status(u16),
    /// No answer the launcher could read (network, TLS, not JSON): why.
    Other(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Status(code) => write!(f, "HTTP {code}"),
            CallError::Other(e) => f.write_str(e),
        }
    }
}

impl Session {
    /// GET `path` (under /api2/json) and return its answer's body, as the API wrote it.
    pub async fn get(&self, path: &str) -> Result<String, CallError> {
        let res = self.client.get(format!("{}{path}", self.base)).header(self.auth.0, &self.auth.1).send().await.map_err(|e| CallError::Other(describe(&e)))?;
        if !res.status().is_success() {
            return Err(CallError::Status(res.status().as_u16()));
        }
        res.text().await.map_err(|e| CallError::Other(e.to_string()))
    }

    /// GET `path` (under /api2/json) and return its `data`, read as `T` ([`api::data`]).
    pub async fn call<T: DeserializeOwned + Default>(&self, path: &str) -> Result<T, CallError> {
        read_data(&self.get(path).await?)
    }

    /// The configured node, or the first one when none is set.
    pub async fn node(&self, h: &HostProfile) -> Result<String, CallError> {
        pick_node(&self.call::<Vec<Node>>("/nodes").await?, h.node.as_deref())
    }
}

/// The `data` of an answer's `body`, read as `T`; an answer that isn't the API's is the call's error.
fn read_data<T: DeserializeOwned + Default>(body: &str) -> Result<T, CallError> {
    api::data(body).map_err(|e| CallError::Other(format!("the Proxmox API's answer isn't what the launcher reads: {e}")))
}

/// The configured node among `/nodes`, or the first one when none is set.
fn pick_node(nodes: &[Node], configured: Option<&str>) -> Result<String, CallError> {
    let names: Vec<&str> = nodes.iter().filter_map(|n| n.node.as_deref()).collect();
    match configured.filter(|n| !n.is_empty()) {
        Some(n) if names.contains(&n) => Ok(n.to_string()),
        Some(n) => Err(CallError::Other(format!("there is no node \"{n}\" on this host (found: {})", names.join(", ")))),
        None => names.first().map(std::string::ToString::to_string).ok_or_else(|| CallError::Other("this user can't see any node".into())),
    }
}

#[cfg(test)]
mod tests;
