//! The Server screen's Tauri commands: list, save, remove and test hosts, what each row shows
//! (running labs, capacity), an AWS account's month-to-date spend, and the host self-test. Each is
//! a thin adapter over the `crate::runtime::server` domain (the setup form's actions are
//! `server::setup`'s, Test connection is `server::connection_test`'s, its rules on the saved hosts
//! are `server::hosts`'). The ones that may reach the keychain are async: off the main thread, as
//! its unlock prompt may wait for the player.

use std::collections::HashMap;

use serde::Serialize;
use tauri::ipc::Channel;

use crate::AppHandle;
use crate::error::Result;
use crate::platform::AppDirs;
use crate::runtime::proxmox::{placement, ssh_key};
use crate::runtime::selftest_report::Event;
use crate::runtime::server::budget::{MonthSpend, account_month_spend, budget_limit};
use crate::runtime::server::capacity::{HostCapacity, capacity};
use crate::runtime::server::checks::TestResult;
use crate::runtime::server::connection_test::test_connection;
use crate::runtime::server::hosts::{running_by_host, set_default_host};
use crate::runtime::server::input::HostInput;
use crate::runtime::server::profile::HostProfile;
use crate::runtime::server::store::{load, update};
use crate::runtime::server::{node_keys, setup};
use crate::runtime::ssh::known_hosts;

/// The saved servers and cloud accounts, and which one is the default.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerList")]
pub struct HostList {
    /// The default host's id, when one is set.
    pub(crate) default: Option<String>,
    /// Every saved server and cloud account.
    pub(crate) hosts: Vec<HostProfile>,
    /// Each account's monthly budget in USD, by id: the ones a cloud launch is checked against
    /// (AWS accounts with a positive limit), whose spend the Cloud page reads.
    pub(crate) budgets: std::collections::BTreeMap<String, f64>,
}

/// The saved servers and cloud accounts, with the default's id.
#[tauri::command]
pub fn server_list(app: AppHandle) -> Result<HostList> {
    host_list(&AppDirs::from(&app))
}

/// [`server_list`], in the app whose folders are `dirs`.
pub(crate) fn host_list(dirs: &AppDirs) -> Result<HostList> {
    let store = load(dirs)?;
    let budgets = store.hosts.iter().filter_map(|h| Some((h.id.clone(), budget_limit(h)?))).collect();
    Ok(HostList { default: store.default, hosts: store.hosts, budgets })
}

/// Creates or updates a host from the setup form (see `server::setup::save`).
#[tauri::command]
pub async fn server_save(app: AppHandle, input: HostInput) -> Result<HostProfile> {
    setup::save(&AppDirs::from(&app), input).await
}

/// A Proxmox server's placement choices, read with the form's sign-in (see
/// `server::setup::placement`).
#[tauri::command]
pub async fn server_proxmox_placement(app: AppHandle, input: HostInput) -> Result<placement::Placement> {
    setup::placement(&AppDirs::from(&app), input).await
}

/// What the form's Proxmox server offers to be confirmed: its certificate's fingerprint and its
/// node's SSH host keys (see `server::setup::identity`).
#[tauri::command]
pub async fn server_identity(app: AppHandle, input: HostInput) -> Result<setup::Identity> {
    setup::identity(&AppDirs::from(&app), &input).await
}

/// Confirms the form's server node's SSH host keys, shown as `fingerprints` (see
/// `server::setup::trust_ssh_keys`).
#[tauri::command]
pub async fn server_trust_ssh_keys(app: AppHandle, input: HostInput, fingerprints: Vec<String>) -> Result<()> {
    setup::trust_ssh_keys(&AppDirs::from(&app), &input, &fingerprints).await
}

/// The SSH host keys of saved server `id`'s node for the player to confirm before a lab runs
/// through it; null when none needs confirming (see `server::node_keys::unconfirmed`).
#[tauri::command]
pub async fn server_node_host_keys(app: AppHandle, id: String) -> Result<Option<known_hosts::Offered>> {
    node_keys::unconfirmed(&AppDirs::from(&app), &id).await
}

/// Confirms saved server `id`'s node keys, shown as `fingerprints` (see
/// `server::node_keys::confirm`).
#[tauri::command]
pub async fn server_trust_node_host_keys(app: AppHandle, id: String, fingerprints: Vec<String>) -> Result<()> {
    node_keys::confirm(&AppDirs::from(&app), &id, &fingerprints).await
}

/// Saves a Proxmox host set up with the player's SSH key (see `server::setup::save_with_ssh_key`).
#[tauri::command]
pub async fn server_save_ssh_key(app: AppHandle, input: HostInput) -> Result<HostProfile> {
    setup::save_with_ssh_key(&AppDirs::from(&app), input).await
}

/// The SSH key the setup offers first: the first of `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa`
/// that exists (its path only).
#[tauri::command]
pub fn server_default_ssh_key() -> Option<String> {
    let home = crate::platform::home_dir()?;
    ssh_key::default_key(&home).map(|p| p.display().to_string())
}

/// Removes a host no lab runs on (see `server::setup::remove`).
#[tauri::command]
pub async fn server_remove(app: AppHandle, id: String) -> Result<()> {
    setup::remove(&AppDirs::from(&app), &id).await
}

/// Sets (or clears, with None) the host VM labs run on by default.
#[tauri::command]
pub fn server_set_default(app: AppHandle, id: Option<String>) -> Result<()> {
    set_default_in(&AppDirs::from(&app), id)
}

/// [`server_set_default`], in the app whose folders are `dirs`.
pub(crate) fn set_default_in(dirs: &AppDirs, id: Option<String>) -> Result<()> {
    update(dirs, |store| set_default_host(store, id))
}

/// "Test connection" of a saved host (see `server::connection_test`).
#[tauri::command]
pub async fn server_test(app: AppHandle, id: String) -> Result<TestResult> {
    test_connection(&AppDirs::from(&app), &id).await
}

/// The launcher's SSH public key (generated on first use). Token-auth Proxmox hosts must
/// authorize it for the token's SSH user, so the launcher can upload the cloud-init snippet
/// over SSH; the setup form shows it for those hosts.
#[tauri::command]
pub async fn server_public_key(app: AppHandle) -> Result<String> {
    public_key_in(&AppDirs::from(&app)).await
}

/// [`server_public_key`], in the app whose folders are `dirs`.
pub(crate) async fn public_key_in(dirs: &AppDirs) -> Result<String> {
    Ok(crate::runtime::ssh::ensure_key(dirs).await?.1)
}

/// How many installed labs are currently running on each host, keyed by host id (as the
/// runner records their runs).
#[tauri::command]
pub async fn server_running_labs(app: AppHandle) -> HashMap<String, u32> {
    running_by_host(&AppDirs::from(&app)).await
}

/// A host's hardware headroom, for its row: `None` for a host that isn't a Proxmox server, an
/// error when it couldn't be read (the row then shows none).
#[tauri::command]
pub async fn server_capacity(app: AppHandle, id: String) -> Result<Option<HostCapacity>> {
    capacity(&AppDirs::from(&app), &id).await
}

/// This month's spend so far on the saved account `id`, which has a budget, and whether it reached
/// it (see `server::budget::account_month_spend`).
#[tauri::command]
pub async fn server_month_to_date_cost(app: AppHandle, id: String) -> Result<MonthSpend> {
    account_month_spend(&AppDirs::from(&app), &id).await
}

/// Runs a real-VM self-test against host `id`, streaming step events (see
/// [`crate::runtime::server_selftest::run`]).
#[tauri::command]
pub async fn server_selftest(app: AppHandle, id: String, events: Channel<Event>) -> Result<()> {
    let dirs = AppDirs::from(&app);
    let data = dirs.data().ok().map(std::path::Path::to_path_buf);
    let ovftool = || crate::machine::tools::ovftool::usable(data);
    crate::runtime::server_selftest::run(&dirs, &id, ovftool, move |e| {
        let _ = events.send(e);
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::providers::Provider;

    /// A public host key (made for these tests) and the fingerprint `ssh-keygen -lf` gives it.
    const NODE_KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIMV/X9GBl8lRmDMg/hSFR41QZH/fgA5PLNatzgxigSSL";
    const NODE_FINGERPRINT: &str = "SHA256:Coh2bRLeV1HsLRUtSn+G8PFvq/E3/excrj4hN1k1YEQ";

    #[tokio::test]
    async fn a_proxmox_servers_identity_is_read_then_its_node_keys_confirmed_through_the_commands() {
        use crate::runtime::server::tls_pin::tests::{FINGERPRINT_A, server_a};
        use crate::testing::tools::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let app = || t.handle().clone();
        // The launcher's own key, already made: nothing runs ssh-keygen.
        let ssh = t.data().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "private").unwrap();
        std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher").unwrap();
        let (port, requests) = server_a().await;
        let input = |provider: &str| -> HostInput {
            serde_json::from_value(serde_json::json!({
                "name": "pve", "provider": provider, "host": "127.0.0.1", "port": port, "username": "root@pam", "insecureTls": true
            }))
            .unwrap()
        };
        let refused = server_identity(app(), input("vmware_esxi")).await.unwrap_err();
        assert!(matches!(&refused, crate::error::Error::Invalid(m) if m == "only Proxmox servers are confirmed by their fingerprints"), "{refused}");
        let tools = Tools::strict_rules(&[("ssh-keyscan", ok(format!("127.0.0.1 ssh-ed25519 {NODE_KEY}\n")))]);
        let shown = server_identity(app(), input("proxmox")).await.unwrap();
        assert_eq!(shown.tls.as_deref(), Some(FINGERPRINT_A), "the certificate's fingerprint, for the learner to compare");
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing but a handshake reached the server");
        let offered = shown.ssh.expect("the node's keys");
        assert_eq!(
            (offered.state, offered.keys.iter().map(|k| k.fingerprint.as_str()).collect::<Vec<_>>()),
            (crate::runtime::ssh::known_hosts::State::New, vec![NODE_FINGERPRINT])
        );
        assert_eq!(shown.ssh_error, None);
        server_trust_ssh_keys(app(), input("proxmox"), vec![NODE_FINGERPRINT.into()]).await.unwrap();
        assert_eq!(std::fs::read_to_string(ssh.join("nodes_known_hosts")).unwrap(), format!("127.0.0.1 ssh-ed25519 {NODE_KEY}\n"));
        assert!(tools.called("ssh-keyscan -T 10 -p 22 -t ed25519,ecdsa,rsa 127.0.0.1"), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn hosts_are_saved_listed_tested_and_removed_through_the_commands() {
        use crate::testing::tools::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let app = || t.handle().clone();
        let input = |id: Option<&str>| -> HostInput {
            serde_json::from_value(serde_json::json!({
                "id": id, "name": "My Azure", "provider": "azure", "host": "westeurope", "username": "00000000-0000-0000-0000-000000000000"
            }))
            .unwrap()
        };
        let saved = server_save(app(), input(None)).await.unwrap();
        assert_eq!(saved.provider, Provider::Azure);
        let edited = server_save(app(), input(Some(&saved.id))).await.unwrap();
        assert_eq!(edited.id, saved.id);
        let list = server_list(app()).unwrap();
        assert_eq!(list.hosts.len(), 1);
        assert_eq!(list.default, None, "a cloud account never becomes the default");
        assert_eq!(server_set_default(app(), Some(saved.id.clone())).unwrap_err().to_string(), "a cloud account can't be the default host");
        server_set_default(app(), None).unwrap();
        // Testing it asks the Azure CLI (no network here).
        let tools = Tools::strict_rules(&crate::testing::tools::azure_signed_in());
        let result = server_test(app(), saved.id.clone()).await.unwrap();
        assert!(tools.called("az account show --subscription 00000000-0000-0000-0000-000000000000"));
        assert!(!result.checks.is_empty());
        assert!(server_capacity(app(), saved.id.clone()).await.unwrap().is_none(), "only Proxmox reports capacity");
        assert_eq!(
            server_month_to_date_cost(app(), saved.id.clone()).await.unwrap_err().to_string(),
            "only an AWS account with a monthly budget reports its spend"
        );
        // A lab running on it keeps it.
        t.lab("web", &[]);
        crate::runtime::test_support::record_run(t.data(), "web", Some(&saved.id), "proxmox", "ready");
        assert_eq!(server_running_labs(app()).await.get(&saved.id), Some(&1));
        assert!(server_remove(app(), saved.id.clone()).await.unwrap_err().to_string().starts_with("1 lab still running"));
        crate::runtime::test_support::record_run(t.data(), "web", Some(&saved.id), "proxmox", "absent");
        server_remove(app(), saved.id.clone()).await.unwrap();
        assert_eq!(server_list(app()).unwrap().hosts, Vec::<HostProfile>::new());
        // The launcher's key, made once.
        let ssh = t.data().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "k").unwrap();
        std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher\n").unwrap();
        assert_eq!(server_public_key(app()).await.unwrap(), "ssh-ed25519 AAAA cyberctf-launcher");
        // The default key offered is one of the usual ones, when the player has one.
        if let Some(path) = server_default_ssh_key() {
            assert!(["id_ed25519", "id_ecdsa", "id_rsa"].iter().any(|n| path.ends_with(n)), "{path}");
        }
    }

    #[tokio::test]
    async fn the_proxmox_setup_commands_reach_their_server() {
        let t = crate::testing::test_app::TestApp::new();
        let form = serde_json::json!({
            "id": null, "name": "P", "provider": "proxmox", "host": "127.0.0.1", "port": 1, "username": "root",
            "datastore": null, "network": null, "node": null, "insecureTls": true, "autoStopHours": null, "password": "pw"
        });
        // Nothing listens on port 1: each read fails and says so.
        let placement = server_proxmox_placement(t.handle().clone(), serde_json::from_value(form.clone()).unwrap()).await;
        assert!(placement.unwrap_err().to_string().starts_with("Couldn't read the server's nodes, storages and bridges"));
        let ssh = server_save_ssh_key(t.handle().clone(), serde_json::from_value(form).unwrap()).await;
        assert_eq!(ssh.unwrap_err().to_string(), "choose the SSH key to sign in with");
    }

    /// Each self-test event's step and state, as the UI gets them.
    fn steps(events: &crate::testing::test_app::Sink) -> Vec<(String, String)> {
        events.messages().iter().map(|e| (e["step"].as_str().unwrap().to_string(), e["state"].as_str().unwrap().to_string())).collect()
    }

    #[tokio::test]
    async fn the_vm_test_runs_on_proxmox_and_esxi_hosts_only() {
        let t = crate::testing::test_app::TestApp::new();
        let tools = crate::testing::tools::Tools::strict_rules(&[]);
        let app = || t.handle().clone();
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());

        // No such host: the first step fails and nothing else runs.
        let events = crate::testing::test_app::Sink::default();
        assert!(server_selftest(app(), "nohost".into(), events.channel()).await.is_err());
        assert_eq!(steps(&events), [pair("connect", "running"), pair("connect", "fail")]);

        // A cloud account: reached, then refused.
        t.host("az1", "azure", serde_json::json!({}));
        let events = crate::testing::test_app::Sink::default();
        let err = server_selftest(app(), "az1".into(), events.channel()).await.unwrap_err();
        assert_eq!(err.to_string(), "the VM test runs only on a Proxmox or ESXi host");
        assert_eq!(steps(&events), [pair("connect", "running"), pair("connect", "ok")]);
        assert_eq!(events.messages()[1]["detail"], "Host az1");

        // AWS is billed: never a test, nothing run for it.
        t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
        let events = crate::testing::test_app::Sink::default();
        let err = server_selftest(app(), "aws1".into(), events.channel()).await.unwrap_err();
        assert!(err.to_string().contains("AWS labs are billed"), "{err}");
        assert_eq!(steps(&events), [pair("connect", "running"), pair("connect", "ok")]);
        assert!(!t.data().join("selftest").join("server").join("aws1").exists(), "no test folder made");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn the_esxi_vm_test_stops_at_its_first_step_without_vagrants_esxi_plugin() {
        let t = crate::testing::test_app::TestApp::new();
        // No Vagrant on this computer: no ESXi plugin either.
        let _tools = crate::testing::tools::Tools::strict_rules(&[("ovftool --version", crate::testing::tools::ok("")), ("vagrant", None)]);
        crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
        t.host("esx", "vmware_esxi", serde_json::json!({ "host": "192.0.2.1", "port": 22, "username": "root" }));
        crate::runtime::server::secrets::set_secret("esx", "pw").unwrap();
        let events = crate::testing::test_app::Sink::default();
        let err = server_selftest(t.handle().clone(), "esx".into(), events.channel()).await.unwrap_err().to_string();
        assert!(err.starts_with("Labs on ESXi run from this computer") && err.contains("Vagrant is not installed"), "{err}");
        // Refused up front: the host isn't reached, and no test lab is made.
        assert_eq!(steps(&events), [("connect".to_string(), "running".to_string()), ("connect".to_string(), "fail".to_string())]);
        assert!(!t.data().join("selftest").join("server").join("esx").exists());
    }
}
