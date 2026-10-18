//! Talking to a Proxmox VE host's API for the Server screen: sign in (API token or
//! user + password), then check what a lab launch will need, so problems show up in
//! "Test connection" with their fix instead of halfway through a launch.
//!
//! A token is recognised by its id, `user@realm!name` (the username field holds it and
//! the keychain holds its secret). The cloud-init snippet still goes over SSH, which a
//! token can't do: token setups SSH as the token's user with the launcher's key.

use std::time::Duration;

use serde_json::Value;

use super::server::HostProfile;
use super::ssh;

mod check;
mod node_ssh;

pub use check::{bridge_is_internal, test};
#[cfg(test)]
use check::{bridge_problem, node_report, outcome, sign_in_report, storage_content, storage_problems};
use node_ssh::ssh_key_problem;
#[cfg(test)]
use node_ssh::{askpass_script, authorize_args, write_askpass};
pub use node_ssh::{authorize_launcher_key, node_login, ssh_user};

const TIMEOUT: Duration = Duration::from_secs(8);

/// The storage `deploy/terraform/proxmox` puts the Debian image and the cloud-init
/// snippet on (its defaults, not configurable from the launcher).
pub const IMAGE_STORAGE: &str = "local";
pub const SNIPPET_STORAGE: &str = "local";

/// `user@realm!name`: a Proxmox API token id rather than a user.
pub fn is_token(username: &str) -> bool {
    username.split_once('!').is_some_and(|(user, name)| user.contains('@') && !name.is_empty())
}

/// The user a token belongs to (`root@pam!cyberctf` -> `root@pam`).
pub fn token_user(username: &str) -> &str {
    username.split_once('!').map(|(u, _)| u).unwrap_or(username)
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
fn find_iface<'a>(ifaces: &'a Value, name: &str) -> Option<&'a Value> {
    ifaces.as_array()?.iter().find(|i| i["iface"].as_str() == Some(name))
}

/// A bridge without physical ports (`bridge_ports` empty or missing).
fn has_no_ports(iface: &Value) -> bool {
    iface["bridge_ports"].as_str().map(str::trim).unwrap_or_default().is_empty()
}

/// A signed-in API client.
pub struct Session {
    client: reqwest::Client,
    base: String,
    /// The header that authenticates each call: a ticket cookie or the token.
    auth: (&'static str, String),
}

#[derive(Debug)]
pub enum SignInError {
    /// The host answered and refused the credentials.
    Rejected,
    /// Couldn't talk to the API (TLS, network, unexpected answer).
    Failed(String),
}

impl std::fmt::Display for SignInError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignInError::Rejected => f.write_str("Proxmox rejected the credentials"),
            SignInError::Failed(e) => f.write_str(e),
        }
    }
}

pub async fn sign_in(h: &HostProfile, secret: &str) -> Result<Session, SignInError> {
    let client =
        reqwest::Client::builder().timeout(TIMEOUT).tls_danger_accept_invalid_certs(h.insecure_tls).build().map_err(|e| SignInError::Failed(e.to_string()))?;
    let base = api_url(h);
    if is_token(&h.username) {
        let session = Session { client, base, auth: ("Authorization", format!("PVEAPIToken={}={secret}", h.username)) };
        // Any authenticated call proves the token.
        return match session.call("/version").await {
            Ok(_) => Ok(session),
            Err(CallError::Status(401)) => Err(SignInError::Rejected),
            Err(e) => Err(SignInError::Failed(e.to_string())),
        };
    }
    let res = client
        .post(format!("{base}/access/ticket"))
        .form(&[("username", h.username.as_str()), ("password", secret)])
        .send()
        .await
        .map_err(|e| SignInError::Failed(describe(&e)))?;
    if res.status().as_u16() == 401 {
        return Err(SignInError::Rejected);
    }
    if !res.status().is_success() {
        return Err(SignInError::Failed(format!("the Proxmox API answered HTTP {}", res.status())));
    }
    let body: Value = res.json().await.map_err(|e| SignInError::Failed(e.to_string()))?;
    let ticket = body["data"]["ticket"].as_str().ok_or_else(|| SignInError::Failed("no ticket in the sign-in answer".into()))?;
    Ok(Session { client, base, auth: ("Cookie", format!("PVEAuthCookie={ticket}")) })
}

/// Turns a reqwest error into one readable line, calling out the usual self-signed case.
pub fn describe(e: &reqwest::Error) -> String {
    let detail = std::iter::successors(Some(e as &dyn std::error::Error), |e| e.source()).map(|e| e.to_string()).collect::<Vec<_>>().join(": ");
    if detail.to_lowercase().contains("certificate") {
        "the host's TLS certificate isn't trusted. Proxmox uses a self-signed certificate by default: turn on \"Self-signed certificate\" for this host, or install a trusted one".into()
    } else {
        detail
    }
}

#[derive(Debug)]
pub enum CallError {
    Status(u16),
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
    /// GET `path` (under /api2/json) and return its `data`.
    pub async fn call(&self, path: &str) -> Result<Value, CallError> {
        let res = self.client.get(format!("{}{path}", self.base)).header(self.auth.0, &self.auth.1).send().await.map_err(|e| CallError::Other(describe(&e)))?;
        if !res.status().is_success() {
            return Err(CallError::Status(res.status().as_u16()));
        }
        let body: Value = res.json().await.map_err(|e| CallError::Other(e.to_string()))?;
        Ok(body["data"].clone())
    }

    /// The configured node, or the first one when none is set.
    pub async fn node(&self, h: &HostProfile) -> Result<String, CallError> {
        pick_node(&self.call("/nodes").await?, h.node.as_deref())
    }
}

/// The configured node among `/nodes`, or the first one when none is set.
fn pick_node(nodes: &Value, configured: Option<&str>) -> Result<String, CallError> {
    let names: Vec<&str> = nodes.as_array().into_iter().flatten().filter_map(|n| n["node"].as_str()).collect();
    match configured.filter(|n| !n.is_empty()) {
        Some(n) if names.contains(&n) => Ok(n.to_string()),
        Some(n) => Err(CallError::Other(format!("there is no node \"{n}\" on this host (found: {})", names.join(", ")))),
        None => names.first().map(|n| n.to_string()).ok_or_else(|| CallError::Other("this user can't see any node".into())),
    }
}

#[cfg(test)]
mod tests;
