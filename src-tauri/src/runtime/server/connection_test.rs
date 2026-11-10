//! "Test connection" of a saved host, whole: the per-provider test (`reachability`), tried again
//! while macOS lets the app onto the local network, then the parts of an SSH-key setup checked
//! again, and a GCP account's labs project.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::SshKeySignIn;
use super::checks::{Check, TestResult};
use super::profile::HostProfile;
use super::reachability::test_host;
use super::secrets::host_secret;
use super::store::load_host;
use crate::error::Result;
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;
use crate::runtime::proxmox::ssh_key;

/// Tests the saved host `id`.
pub(crate) async fn test_connection(dirs: &AppDirs, id: &str) -> Result<TestResult> {
    let host = load_host(dirs, id)?;
    // CLI-credential hosts keep no secret; the CLI resolves its own credentials.
    let password = host_secret(&host)?;
    let mut result = test_host(dirs, &host, &password).await;
    if !result.reachable && cfg!(target_os = "macos") {
        for secs in retry_delays(launched_recently()) {
            tokio::time::sleep(Duration::from_secs(*secs)).await;
            result = test_host(dirs, &host, &password).await;
            if result.reachable {
                break;
            }
        }
    }
    // Set up with an SSH key: each part of that setup, then the API's answer with the token.
    if let Some(key) = host.ssh_key.clone().filter(|_| host.provider == Provider::Proxmox) {
        result = result.with_sign_in_checks(ssh_key_checks(dirs, &host, &key).await);
    }
    // GCP: the labs project (created on the first test), which needs a free billing slot.
    if host.provider == Provider::Gcp && result.ok {
        match super::gcp::labs_project(dirs, id).await {
            Ok(p) => result.checks.push(Check::ok("Labs project", format!("Labs run in the project {p}."))),
            Err(e) => {
                result.checks.push(Check::fail("Labs project", e.to_string()));
                result.ok = false;
            }
        }
    }
    Ok(result)
}

/// How long to wait before each new try of a host macOS answered "no route to host" for (in
/// seconds). It does for the app's first connection to a LAN host after a launch, while it checks
/// the Local Network permission; the next one goes through. One retry keeps every freshly opened
/// app from listing reachable servers as Unreachable. Right after an update (a new binary) that
/// takes longer, so for the app's first minute (`recent`) it keeps trying for about 10 s; later,
/// an unreachable host is reported after one retry.
fn retry_delays(recent: bool) -> &'static [u64] {
    if recent { &[2, 3, 5] } else { &[2] }
}

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Notes the app's start, for `launched_recently`.
pub fn mark_started() {
    STARTED.get_or_init(Instant::now);
}

/// Whether the app started less than a minute ago.
fn launched_recently() -> bool {
    STARTED.get_or_init(Instant::now).elapsed() < Duration::from_secs(60)
}

/// "Test connection" of a host set up with an SSH key: each part of the setup, checked again
/// (SSH with the player's key, the launcher's key in `authorized_keys` — added back if it went),
/// before the API's own answer with the token.
async fn ssh_key_checks(dirs: &AppDirs, host: &HostProfile, key: &SshKeySignIn) -> Vec<Check> {
    let mut checks = Vec::new();
    let login = format!("{}@{}:{}", key.user, host.host, key.port);
    let authorized = match crate::runtime::ssh::ensure_key(dirs).await {
        Ok((launcher, public)) => ssh_key::authorize(&host.host, key, &crate::runtime::ssh::node_known_hosts(&launcher), &public).await,
        Err(e) => Err(ssh_key::Failure { part: ssh_key::Part::Key, detail: format!("the launcher's own key isn't ready: {e}") }),
    };
    match &authorized {
        Err(f) if f.part == ssh_key::Part::Ssh => checks.push(Check::fail("SSH with your key", f.to_string())),
        _ => checks.push(Check::ok("SSH with your key", format!("Signed in to {login} with {}.", key.path))),
    }
    let made = super::setup::made_with_key(&host.id);
    checks.push(Check::ok("API token", made.unwrap_or_else(|| format!("Signs in with the token {}, made on the node with your key.", host.username))));
    checks.push(match authorized {
        Ok(true) => Check::ok("Launcher key", format!("Added the launcher's key to {}'s authorized_keys.", key.user)),
        Ok(false) => Check::ok("Launcher key", format!("The launcher's key is in {}'s authorized_keys.", key.user)),
        Err(f) if f.part == ssh_key::Part::Ssh => Check::fail("Launcher key", "Not checked: SSH with your key failed."),
        Err(f) => Check::fail("Launcher key", f.to_string()),
    });
    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_is_recent_right_after_marking_it() {
        mark_started();
        assert!(launched_recently());
    }

    #[test]
    fn a_fresh_app_tries_a_host_for_longer_than_one_open_a_while() {
        assert_eq!(retry_delays(true), [2, 3, 5]);
        assert_eq!(retry_delays(false), [2]);
    }

    #[tokio::test]
    async fn a_host_is_tested_by_its_provider_and_an_unsaved_one_is_an_error() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let az = t.host("az1", "azure", serde_json::json!({}));
        let tools = Tools::rules(&[("account show", ok("Pay-As-You-Go\n"))]);
        let result = test_connection(&t.dirs(), &az).await.unwrap();
        assert!(tools.called("az account show"), "{:?}", tools.calls());
        assert!(!result.checks.is_empty());
        assert!(test_connection(&t.dirs(), "nope").await.is_err());
    }

    #[tokio::test]
    async fn an_ssh_key_setup_is_checked_again_part_by_part() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let ssh = t.data().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "k").unwrap();
        std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher\n").unwrap();
        let id = t.host(
            "pve1",
            "proxmox",
            serde_json::json!({ "host": "10.0.0.50", "port": 8006, "username": "root@pam!cyberctf", "sshKey": { "path": "/home/me/.ssh/id_ed25519", "user": "root", "port": 22 } }),
        );
        let host = load_host(&t.dirs(), &id).unwrap();
        // The key already there: only the authorization is run, and every part passes.
        let tools = Tools::rules(&[("authorized_keys", ok("present"))]);
        let checks = ssh_key_checks(&t.dirs(), &host, host.ssh_key.as_ref().unwrap()).await;
        let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["SSH with your key", "API token", "Launcher key"]);
        assert!(checks.iter().all(|c| c.state == super::super::checks::CheckState::Ok), "{checks:?}");
        assert!(checks[1].detail.contains("root@pam!cyberctf") && checks[2].detail.contains("is in root's authorized_keys"), "{checks:?}");
        assert_eq!(tools.calls().len(), 1);
    }
}
