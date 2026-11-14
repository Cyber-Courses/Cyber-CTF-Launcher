//! The SSH host keys of the server nodes the launcher signs in to (a Proxmox node: its key setup,
//! the snippet upload, the jump to an internal lab bridge), trusted on first use: the keys the
//! node offers are read (`ssh-keyscan`) and shown to the learner by their SHA-256 fingerprints,
//! as `ssh-keygen -lf` and the node's own console show them; once confirmed they are written to
//! the launcher's own `known_hosts` files, and every SSH to the node checks them strictly. A node
//! offering other keys is refused until the learner confirms the new ones.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::platform::exec::run_env_timed;

/// How long reading a node's keys may take.
const SCAN_LIMIT: Duration = Duration::from_secs(20);

/// The key types read, strongest first.
const KEY_TYPES: &str = "ed25519,ecdsa,rsa";

/// One host key of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostKey {
    /// Its type (`ssh-ed25519`).
    algorithm: String,
    /// Its public key, base64 as `known_hosts` writes it.
    blob: String,
}

impl HostKey {
    /// Its SHA-256 fingerprint, as OpenSSH writes it (`SHA256:` and unpadded base64).
    fn fingerprint(&self) -> Option<String> {
        let raw = base64::engine::general_purpose::STANDARD.decode(&self.blob).ok()?;
        Some(format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(raw))))
    }
}

/// A node's key as the learner is shown it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SshHostKey")]
pub struct Shown {
    /// Its type (`ssh-ed25519`).
    pub algorithm: String,
    /// Its SHA-256 fingerprint (`SHA256:…`).
    pub fingerprint: String,
}

/// Whether the keys a node offers are the ones confirmed for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SshHostKeyState")]
pub enum State {
    /// None was confirmed for it yet.
    New,
    /// They are.
    Confirmed,
    /// Others were: the node was reinstalled, or the connection is intercepted.
    Changed,
}

/// The keys a node offers and whether they are the confirmed ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SshHostKeys")]
pub struct Offered {
    /// The node's address and SSH port, as `known_hosts` names it (`pve.lan`, `[pve.lan]:2222`).
    pub pattern: String,
    /// The keys, strongest first.
    pub keys: Vec<Shown>,
    /// Whether they are the confirmed ones.
    pub state: State,
}

/// How `known_hosts` names `host` reached on `port`.
pub fn pattern(host: &str, port: u16) -> String {
    if port == 22 { host.to_owned() } else { format!("[{host}]:{port}") }
}

/// The keys of the lines of `text` (a scan, or `ssh-keygen -F`'s answer) that name `pattern`.
fn keys_in(text: &str, pattern: &str) -> Vec<HostKey> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with('@'))
        .filter_map(|l| {
            let mut fields = l.split_whitespace();
            let (hosts, algorithm, blob) = (fields.next()?, fields.next()?, fields.next()?);
            // A scan names the host; `ssh-keygen -F` may answer a hashed name (`|1|…`).
            let ours = hosts.starts_with("|1|") || hosts.split(',').any(|n| n == pattern);
            ours.then(|| HostKey { algorithm: algorithm.to_owned(), blob: blob.to_owned() })
        })
        .collect()
}

/// The keys `host` offers on `port`.
async fn scan(host: &str, port: u16) -> Result<Vec<HostKey>> {
    if !super::safe_token(host) {
        return Err(Error::Invalid("unexpected SSH host".into()));
    }
    let port_arg = port.to_string();
    let out = run_env_timed("ssh-keyscan", &["-T", "10", "-p", &port_arg, "-t", KEY_TYPES, host], None, &[], SCAN_LIMIT)
        .await
        .map_err(|e| Error::Invalid(format!("couldn't read the SSH host keys of {host} ({e})")))?;
    let mut keys = keys_in(&out, &pattern(host, port));
    keys.dedup();
    if keys.is_empty() {
        return Err(Error::Invalid(format!("{host} offered no SSH host key on port {port}")));
    }
    Ok(keys)
}

/// The keys confirmed for `pattern` in `file`; none for a file not written yet.
async fn confirmed(file: &Path, pattern: &str) -> Result<Vec<HostKey>> {
    if !file.is_file() {
        return Ok(Vec::new());
    }
    let path = file.to_string_lossy();
    match run_env_timed("ssh-keygen", &["-F", pattern, "-f", &path], None, &[], SCAN_LIMIT).await {
        Ok(out) => Ok(keys_in(&out, pattern)),
        // Not found: `ssh-keygen -F` fails without a word.
        Err(Error::CommandFailed { stderr, .. }) if stderr.trim().is_empty() => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Whether the keys `offered` are the ones confirmed (`known`).
fn state(offered: &[HostKey], known: &[HostKey]) -> State {
    match known {
        [] => State::New,
        known if known.iter().all(|k| offered.contains(k)) => State::Confirmed,
        _ => State::Changed,
    }
}

/// The keys `host` offers on `port`, for the learner to confirm, and whether they are the ones
/// confirmed in `file` already.
pub async fn offered(host: &str, port: u16, file: &Path) -> Result<Offered> {
    let pattern = pattern(host, port);
    let keys = scan(host, port).await?;
    let known = confirmed(file, &pattern).await?;
    let shown = keys.iter().filter_map(|k| Some(Shown { algorithm: k.algorithm.clone(), fingerprint: k.fingerprint()? })).collect();
    Ok(Offered { state: state(&keys, &known), keys: shown, pattern })
}

/// Confirms the keys `host` offers on `port`, which the learner was shown as `fingerprints`: read
/// again, they must be exactly those (a node that changed its keys meanwhile is refused), then
/// they replace any keys `files` had for it.
pub async fn confirm(host: &str, port: u16, fingerprints: &[String], files: &[PathBuf]) -> Result<()> {
    let pattern = pattern(host, port);
    let keys = scan(host, port).await?;
    let mut now: Vec<String> = keys.iter().filter_map(HostKey::fingerprint).collect();
    let mut shown = fingerprints.to_vec();
    now.sort();
    shown.sort();
    if now != shown {
        return Err(Error::Invalid(format!("the SSH host keys of {host} changed while you were confirming them: check them again")));
    }
    let lines = keys.iter().fold(String::new(), |mut out, k| {
        let _ = writeln!(out, "{pattern} {} {}", k.algorithm, k.blob);
        out
    });
    for file in files {
        forget(file, &pattern).await?;
        if let Some(dir) = file.parent() {
            crate::platform::private_fs::create_dir(dir).map_err(Error::Io)?;
        }
        // The other hosts' keys kept, this one's (forgotten above) written after them.
        let kept = match std::fs::read_to_string(file) {
            Ok(text) if text.is_empty() || text.ends_with('\n') => text,
            Ok(text) => text + "\n",
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Error::Io(e)),
        };
        crate::platform::private_fs::write(file, kept + &lines).map_err(Error::Io)?;
    }
    Ok(())
}

/// Removes the keys `file` has for `pattern`, hashed or not (`ssh-keygen -R`, which keeps the
/// previous file as `.old`).
async fn forget(file: &Path, pattern: &str) -> Result<()> {
    if !file.is_file() {
        return Ok(());
    }
    run_env_timed("ssh-keygen", &["-R", pattern, "-f", &file.to_string_lossy()], None, &[], SCAN_LIMIT).await.map(drop)
}

/// The error for an SSH that failed: when ssh refused the node's host key (none confirmed, or
/// another one), the learner's error naming the node; anything else as it is.
pub fn refused_key(host: &str, e: Error) -> Error {
    match &e {
        Error::CommandFailed { stderr, .. } if stderr.contains("Host key verification failed") => {
            Error::coded(crate::error::Code::SshHostKeyRefused { host: host.to_owned() })
        }
        _ => e,
    }
}

#[cfg(test)]
mod tests;
