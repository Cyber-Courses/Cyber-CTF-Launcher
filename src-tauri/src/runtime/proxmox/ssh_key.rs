//! Setting up a Proxmox node with an SSH key the player already has: the launcher signs in to
//! the node over SSH with that key and, there, makes an API token for itself (`pveum user token
//! add`) and lets its own key into the user's `authorized_keys`. The setup then runs exactly like
//! a token one: the token for the API, the launcher's key for SSH and the jump to lab VMs. The
//! setup itself is isothrum-machine's (`place::proxmox::bootstrap`); the launcher keeps which key
//! the player picked and what they are told.
//!
//! Only those two commands ever run on the node, and only the key's path is ever used (ssh
//! reads the key itself): its contents are never read, kept or logged.

use std::path::{Path, PathBuf};

use isothrum_machine::place::proxmox::bootstrap::{self, BootstrapError};
use isothrum_machine::place::ssh::Target;

use super::ssh;
use crate::runtime::server::SshKeySignIn;

/// The keys tried, in order, when the player hasn't picked one (under `~/.ssh`).
const DEFAULT_KEYS: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];

/// What the launcher's token names begin with.
const TOKEN_PREFIX: &str = "cyberctf";

/// The first of `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa` that exists under `home`.
pub fn default_key(home: &Path) -> Option<PathBuf> {
    DEFAULT_KEYS.iter().map(|name| home.join(".ssh").join(name)).find(|p| p.is_file())
}

/// The launcher's token name on the node: `cyberctf-` and a short id of this launcher (the first
/// 8 hex digits of its public key's SHA-256), so two machines set up on the same node each keep
/// their own token, and setting up again from this one replaces its own.
pub fn token_name(launcher_public: &str) -> String {
    bootstrap::token_name(TOKEN_PREFIX, launcher_public)
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

impl From<BootstrapError> for Failure {
    fn from(e: BootstrapError) -> Failure {
        let part = match e.part {
            bootstrap::Part::Token => Part::Token,
            bootstrap::Part::Key => Part::Key,
            _ => Part::Ssh,
        };
        // What the node or ssh said, without the runner's exit code after it.
        let detail = e.detail.split(" (exited with code").next().unwrap_or_default().trim().to_owned();
        Failure { part, detail }
    }
}

/// The node reached over SSH with the player's key, its host keys the ones confirmed in
/// `known_hosts`.
pub fn target(host: &str, key: &SshKeySignIn, known_hosts: &Path) -> Target {
    Target::new(host, key.user.as_str(), PathBuf::from(&key.path), known_hosts.to_path_buf()).on_port(key.port)
}

/// Lets the launcher's key (`public`) into the node's `authorized_keys` for the key's user, over
/// SSH with the player's key; `true` when it was added, `false` when it was there already. The
/// node's host key must be one the learner confirmed in the launcher's node `known_hosts`
/// (`known_hosts`).
pub async fn authorize(host: &str, key: &SshKeySignIn, known_hosts: &Path, public: &str) -> Result<bool, Failure> {
    bootstrap::authorize(&ssh::machine(), &target(host, key, known_hosts), public).await.map_err(Failure::from)
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
    let made = bootstrap::bootstrap(&ssh::machine(), &target(host, key, known_hosts), public, &token_name(public)).await?;
    Ok(Made { token_id: made.token.id, secret: made.token.secret, key_added: made.key_added })
}

#[cfg(test)]
mod tests {
    use super::*;
    use isothrum_runner::fake::{CallView as _, Reply};

    use crate::runtime::providers::testing::fake;
    use crate::runtime::test_support::node_over_ssh;

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
        let tools = fake(node_over_ssh(vec![("authorized_keys", Reply::ok("added\n")), ("pveum user token", Reply::ok(ANSWER))]));
        let made = bootstrap("10.0.0.50", &sign_in(), Path::new("/data/ssh/nodes_known_hosts"), PUBLIC).await.unwrap();
        assert_eq!(made, Made { token_id: "root@pam!cyberctf-0a1b2c3d".into(), secret: "6f1d2c3b-aaaa-bbbb-cccc-0123456789ab".into(), key_added: true });
        let (calls, lines) = (tools.calls(), tools.lines());
        assert_eq!(calls.len(), 2, "only the two node commands run: {lines:?}");
        assert!(
            calls
                .iter()
                .all(|c| c.args.starts_with(&["-i".to_owned(), "/Users/me/.ssh/id_ed25519".to_owned(), "-p".to_owned(), "22".to_owned()])
                    && c.has("root@10.0.0.50")),
            "{lines:?}"
        );
        let command = |c: &isothrum_runner::fake::Call| c.args.last().cloned().unwrap_or_default();
        assert!(
            command(&calls[0]).contains("authorized_keys")
                && command(&calls[1]).contains(&format!("pveum user token add 'root@pam' '{}' --privsep 0", token_name(PUBLIC))),
            "{lines:?}"
        );
        assert!(calls.iter().all(|c| c.has("BatchMode=yes")), "never prompts");
    }

    #[tokio::test]
    async fn a_key_already_authorized_is_not_added_twice() {
        let _tools = fake(node_over_ssh(vec![("authorized_keys", Reply::ok("present\n")), ("pveum user token", Reply::ok(ANSWER))]));
        let made = bootstrap("10.0.0.50", &sign_in(), Path::new("/kh"), PUBLIC).await.unwrap();
        assert!(!made.key_added);
    }

    #[tokio::test]
    async fn each_failure_names_its_part() {
        async fn authorize_failing(stderr: &str) -> Failure {
            let _tools = fake(node_over_ssh(vec![("authorized_keys", Reply::fail(1, stderr))]));
            bootstrap("10.0.0.50", &sign_in(), Path::new("/kh"), PUBLIC).await.unwrap_err()
        }
        let refused = authorize_failing("root@10.0.0.50: Permission denied (publickey,password).").await;
        assert_eq!(refused.part, Part::Ssh);
        assert!(refused.to_string().starts_with("Couldn't sign in to the node over SSH with your key: root@10.0.0.50: Permission denied"));
        assert_eq!(authorize_failing("bash: /root/.ssh/authorized_keys: Read-only file system").await.part, Part::Key);

        let _tools =
            fake(node_over_ssh(vec![("authorized_keys", Reply::ok("present")), ("pveum user token", Reply::fail(1, "user 'root@pam' does not exist"))]));
        let token = bootstrap("10.0.0.50", &sign_in(), Path::new("/kh"), PUBLIC).await.unwrap_err();
        assert_eq!(token, Failure { part: Part::Token, detail: "user 'root@pam' does not exist".into() });
    }
}
