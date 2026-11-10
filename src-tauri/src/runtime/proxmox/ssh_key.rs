//! Setting up a Proxmox node with an SSH key the player already has: the launcher signs in to
//! the node over SSH with that key and, there, makes an API token for itself (`pveum user token
//! add`) and lets its own key into the user's `authorized_keys`. The setup then runs exactly like
//! a token one: the token for the API, the launcher's key for SSH and the jump to lab VMs.
//!
//! Only those two commands ever run on the node, and only the key's path is ever used (ssh
//! reads the key itself): its contents are never read, kept or logged.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::{is_token, ssh};
use crate::error::Error;
use crate::runtime::server::SshKeySignIn;

/// The most one SSH call to the node may take.
const SSH_TIMEOUT: Duration = Duration::from_secs(60);

/// The keys tried, in order, when the player hasn't picked one (under `~/.ssh`).
const DEFAULT_KEYS: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];

/// The first of `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa` that exists under `home`.
pub fn default_key(home: &Path) -> Option<PathBuf> {
    DEFAULT_KEYS.iter().map(|name| home.join(".ssh").join(name)).find(|p| p.is_file())
}

/// The launcher's token name on the node: `cyberctf-` and a short id of this launcher (the first
/// 8 hex digits of its public key's SHA-256), so two machines set up on the same node each keep
/// their own token, and setting up again from this one replaces its own.
pub fn token_name(launcher_public: &str) -> String {
    let digest = Sha256::digest(launcher_public.trim().as_bytes());
    format!("cyberctf-{}", crate::platform::hex(&digest[..4]))
}

/// The node command that (re)makes the token: an older token of the same name is removed first,
/// since Proxmox shows a token's secret only when it is made. Privilege separation off: the token
/// acts as its user (root can create VMs, upload snippets).
pub(crate) fn token_command(user: &str, name: &str) -> String {
    format!("pveum user token remove {user}@pam {name} >/dev/null 2>&1; pveum user token add {user}@pam {name} --privsep 0 --output-format json")
}

/// The node command that adds the launcher's public key to the user's `authorized_keys` unless
/// it is there already, printing `added` or `present`.
pub(crate) fn authorize_command(public: &str) -> String {
    let key = ssh::sh_quote(public.trim());
    format!(
        "umask 077; mkdir -p ~/.ssh; if grep -qxF {key} ~/.ssh/authorized_keys 2>/dev/null; then echo present; else echo {key} >> ~/.ssh/authorized_keys && echo added; fi"
    )
}

/// The token id and secret in `pveum user token add --output-format json`'s answer.
pub(crate) fn parse_token(out: &str) -> Result<(String, String), String> {
    let json = out.lines().find(|l| l.trim_start().starts_with('{')).ok_or_else(|| format!("pveum gave no token: {}", out.trim()))?;
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("pveum's answer isn't JSON ({e})"))?;
    let id = v["full-tokenid"].as_str().unwrap_or_default().to_string();
    let secret = v["value"].as_str().unwrap_or_default().to_string();
    let secret_ok = !secret.is_empty() && secret.len() <= 128 && secret.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !is_token(&id) || !secret_ok {
        return Err("pveum's answer has no token id and secret".into());
    }
    Ok((id, secret))
}

/// Which part of the setup failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// Signing in to the node over SSH with the player's key.
    Ssh,
    /// Making the API token.
    Token,
    /// Letting the launcher's key in.
    Key,
}

/// A failed part and why.
#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    /// The part that failed.
    pub part: Part,
    /// Why, in the words of the tool that failed.
    pub detail: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.part {
            Part::Ssh => write!(f, "Couldn't sign in to the node over SSH with your key: {}", self.detail),
            Part::Token => write!(f, "Signed in over SSH, but making the launcher's API token failed: {}", self.detail),
            Part::Key => write!(f, "Signed in over SSH, but adding the launcher's key to authorized_keys failed: {}", self.detail),
        }
    }
}

/// What the node said: the stderr of a failed ssh call, else the error.
fn detail(e: &Error) -> String {
    match e {
        Error::CommandFailed { stderr, .. } => stderr.trim().to_string(),
        e => e.to_string(),
    }
}

/// Whether ssh itself failed (no connection, the key refused, an unknown host key), rather than
/// the command it ran on the node.
fn ssh_failed(stderr: &str) -> bool {
    [
        "Permission denied",
        "Connection refused",
        "Connection timed out",
        "Operation timed out",
        "No route to host",
        "Could not resolve",
        "Host key verification failed",
        "Too many authentication failures",
        "Load key",
        "Connection closed",
        "timed out after",
        "kex_exchange_identification",
    ]
    .iter()
    .any(|m| stderr.contains(m))
}

/// The node reached over SSH with the player's key.
pub fn target(host: &str, key: &SshKeySignIn) -> ssh::Target {
    ssh::Target { port: key.port, ..ssh::Target::direct(host, key.user.as_str(), PathBuf::from(&key.path)) }
}

/// Lets the launcher's key (`public`) into the node's `authorized_keys` for the key's user, over
/// SSH with the player's key; `true` when it was added, `false` when it was there already. The
/// node's host key is pinned in the launcher's node `known_hosts` (`known_hosts`).
pub async fn authorize(host: &str, key: &SshKeySignIn, known_hosts: &Path, public: &str) -> Result<bool, Failure> {
    match target(host, key).exec_timed(known_hosts, &authorize_command(public), SSH_TIMEOUT).await {
        Ok(out) if out.lines().any(|l| l.trim() == "added") => Ok(true),
        Ok(out) if out.lines().any(|l| l.trim() == "present") => Ok(false),
        Ok(out) => Err(Failure { part: Part::Key, detail: format!("unexpected answer: {}", out.trim()) }),
        Err(e) => {
            let detail = detail(&e);
            Err(Failure { part: if ssh_failed(&detail) { Part::Ssh } else { Part::Key }, detail })
        }
    }
}

/// The token made on the node, and whether the launcher's key was added (or there already).
#[derive(Debug, PartialEq, Eq)]
pub struct Made {
    /// The token's id, `user@realm!name`.
    pub token_id: String,
    /// The token's secret.
    pub secret: String,
    /// Whether the launcher's SSH key is let in on the node (added now, or there already).
    pub key_added: bool,
}

/// Sets the node up with the player's key: lets the launcher's key in (first: it also proves SSH
/// works before anything is changed), then (re)makes the launcher's token.
pub async fn bootstrap(host: &str, key: &SshKeySignIn, known_hosts: &Path, public: &str) -> Result<Made, Failure> {
    let key_added = authorize(host, key, known_hosts, public).await?;
    let command = token_command(&key.user, &token_name(public));
    let out = target(host, key).exec_timed(known_hosts, &command, SSH_TIMEOUT).await.map_err(|e| {
        let detail = detail(&e);
        Failure { part: if ssh_failed(&detail) { Part::Ssh } else { Part::Token }, detail }
    })?;
    let (token_id, secret) = parse_token(&out).map_err(|detail| Failure { part: Part::Token, detail })?;
    Ok(Made { token_id, secret, key_added })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

    const PUBLIC: &str = "ssh-ed25519 AAAAC3launcher cyberctf-launcher";

    fn sign_in() -> SshKeySignIn {
        SshKeySignIn { path: "/Users/me/.ssh/id_ed25519".into(), user: "root".into(), port: 22 }
    }

    const ANSWER: &str = r#"{"full-tokenid":"root@pam!cyberctf-0a1b2c3d","info":{"privsep":"0"},"value":"6f1d2c3b-aaaa-bbbb-cccc-0123456789ab"}"#;

    #[test]
    fn the_token_is_named_after_this_launcher() {
        let name = token_name(PUBLIC);
        assert!(name.starts_with("cyberctf-") && name.len() == "cyberctf-".len() + 8, "{name}");
        assert_eq!(token_name(&format!("{PUBLIC}\n")), name, "stable for the same key");
        assert_ne!(token_name("ssh-ed25519 BBBB other"), name);
    }

    #[test]
    fn the_node_commands_are_the_two_expected() {
        assert_eq!(
            token_command("root", "cyberctf-0a1b2c3d"),
            "pveum user token remove root@pam cyberctf-0a1b2c3d >/dev/null 2>&1; pveum user token add root@pam cyberctf-0a1b2c3d --privsep 0 --output-format json"
        );
        let add = authorize_command(PUBLIC);
        assert!(add.contains("grep -qxF 'ssh-ed25519 AAAAC3launcher cyberctf-launcher' ~/.ssh/authorized_keys"), "{add}");
        assert!(add.contains("echo 'ssh-ed25519 AAAAC3launcher cyberctf-launcher' >> ~/.ssh/authorized_keys"), "{add}");
        // A quote in the key can't end the quoting.
        assert!(authorize_command("ssh-ed25519 AA'; rm -rf ~ #").contains(r"'ssh-ed25519 AA'\''; rm -rf ~ #'"));
    }

    #[test]
    fn the_token_comes_from_pveums_json() {
        assert_eq!(parse_token(ANSWER).unwrap(), ("root@pam!cyberctf-0a1b2c3d".into(), "6f1d2c3b-aaaa-bbbb-cccc-0123456789ab".into()));
        assert_eq!(parse_token(&format!("warning: something\n{ANSWER}\n")).unwrap().0, "root@pam!cyberctf-0a1b2c3d");
        assert!(parse_token("").is_err());
        assert!(parse_token(r#"{"full-tokenid":"root@pam","value":"x"}"#).is_err());
        assert!(parse_token(r#"{"full-tokenid":"root@pam!n","value":"a b"}"#).is_err());
    }

    #[test]
    fn the_default_key_is_the_first_that_exists() {
        let home = std::env::temp_dir().join(format!("cyberctf-home-{}", rand::random::<u32>()));
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        assert_eq!(default_key(&home), None);
        std::fs::write(home.join(".ssh/id_rsa"), "").unwrap();
        assert_eq!(default_key(&home), Some(home.join(".ssh/id_rsa")));
        std::fs::write(home.join(".ssh/id_ecdsa"), "").unwrap();
        assert_eq!(default_key(&home), Some(home.join(".ssh/id_ecdsa")));
        std::fs::write(home.join(".ssh/id_ed25519"), "").unwrap();
        assert_eq!(default_key(&home), Some(home.join(".ssh/id_ed25519")));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[tokio::test]
    async fn bootstrap_authorizes_the_launcher_key_then_makes_the_token() {
        let tools = Tools::rules(&[("authorized_keys", ok("added\n")), ("pveum user token", ok(ANSWER))]);
        let made = bootstrap("10.0.0.50", &sign_in(), Path::new("/data/ssh/nodes_known_hosts"), PUBLIC).await.unwrap();
        assert_eq!(made, Made { token_id: "root@pam!cyberctf-0a1b2c3d".into(), secret: "6f1d2c3b-aaaa-bbbb-cccc-0123456789ab".into(), key_added: true });
        let calls = tools.calls();
        assert_eq!(calls.len(), 2, "only the two node commands run: {calls:?}");
        assert!(calls.iter().all(|c| c.starts_with("ssh -i /Users/me/.ssh/id_ed25519 -p 22 ") && c.contains(" root@10.0.0.50 ")), "{calls:?}");
        assert!(calls[0].contains("authorized_keys") && calls[1].contains(&token_command("root", &token_name(PUBLIC))), "{calls:?}");
        assert!(calls.iter().all(|c| c.contains("BatchMode=yes")), "never prompts");
    }

    #[tokio::test]
    async fn a_key_already_authorized_is_not_added_twice() {
        let _tools = Tools::rules(&[("authorized_keys", ok("present\n")), ("pveum user token", ok(ANSWER))]);
        let made = bootstrap("10.0.0.50", &sign_in(), Path::new("/kh"), PUBLIC).await.unwrap();
        assert!(!made.key_added);
        // The command itself only appends when grep finds no identical line.
        let cmd = authorize_command(PUBLIC);
        let (check, add) = cmd.split_once("then echo present; else").unwrap();
        assert!(check.contains("if grep -qxF") && add.contains(">> ~/.ssh/authorized_keys"));
    }

    #[tokio::test]
    async fn each_failure_names_its_part() {
        async fn authorize_failing(stderr: &str) -> Failure {
            let _tools = Tools::rules(&[("authorized_keys", fail(stderr))]);
            bootstrap("10.0.0.50", &sign_in(), Path::new("/kh"), PUBLIC).await.unwrap_err()
        }
        let refused = authorize_failing("root@10.0.0.50: Permission denied (publickey,password).").await;
        assert_eq!(refused.part, Part::Ssh);
        assert!(refused.to_string().starts_with("Couldn't sign in to the node over SSH with your key: root@10.0.0.50: Permission denied"));
        assert_eq!(authorize_failing("bash: /root/.ssh/authorized_keys: Read-only file system").await.part, Part::Key);

        let _tools = Tools::rules(&[("authorized_keys", ok("present")), ("pveum user token", fail("user 'root@pam' does not exist"))]);
        let token = bootstrap("10.0.0.50", &sign_in(), Path::new("/kh"), PUBLIC).await.unwrap_err();
        assert_eq!(token, Failure { part: Part::Token, detail: "user 'root@pam' does not exist".into() });
    }
}
