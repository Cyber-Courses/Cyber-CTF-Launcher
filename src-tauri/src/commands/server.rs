//! The Server screen's Tauri commands: list, save, remove and test hosts, what each row shows
//! (running labs, capacity), an AWS account's month-to-date spend, and the host self-test. Each is
//! a thin adapter over the `crate::runtime::server` domain (the setup form's actions are
//! `server::setup`'s, Test connection is `server::connection_test`'s, its rules on the saved hosts
//! are `server::hosts`'). The ones that may reach the keychain are async: off the main thread, as
//! its unlock prompt may wait for the player.

use std::collections::HashMap;

use serde::Serialize;
use tauri::Manager;
use tauri::ipc::Channel;

use crate::AppHandle;
use crate::error::Result;
use crate::platform::AppDirs;
use crate::runtime::proxmox::{placement, ssh_key};
use crate::runtime::selftest_report::Event;
use crate::runtime::server::budget::account_month_to_date_cost;
use crate::runtime::server::capacity::{HostCapacity, capacity};
use crate::runtime::server::checks::TestResult;
use crate::runtime::server::connection_test::test_connection;
use crate::runtime::server::hosts::{running_by_host, set_default_host};
use crate::runtime::server::input::HostInput;
use crate::runtime::server::profile::HostProfile;
use crate::runtime::server::setup;
use crate::runtime::server::store::{load, update};

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerList")]
pub struct HostList {
    default: Option<String>,
    hosts: Vec<HostProfile>,
}

#[tauri::command]
pub fn server_list(app: AppHandle) -> Result<HostList> {
    let store = load(&AppDirs::from(&app))?;
    Ok(HostList { default: store.default, hosts: store.hosts })
}

/// Creates or updates a host from the setup form (see `server::setup::save`).
#[tauri::command]
pub async fn server_save(app: AppHandle, input: HostInput) -> Result<HostProfile> {
    setup::save(&AppDirs::from(&app), input)
}

/// A Proxmox server's placement choices, read with the form's sign-in (see
/// `server::setup::placement`).
#[tauri::command]
pub async fn server_proxmox_placement(app: AppHandle, input: HostInput) -> Result<placement::Placement> {
    setup::placement(&AppDirs::from(&app), input).await
}

/// Saves a Proxmox host set up with the player's SSH key (see `server::setup::save_with_ssh_key`).
#[tauri::command]
pub async fn server_save_ssh_key(app: AppHandle, input: HostInput) -> Result<HostProfile> {
    setup::save_with_ssh_key(&AppDirs::from(&app), input).await
}

/// The SSH key the setup offers first: the first of `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa`
/// that exists (its path only).
#[tauri::command]
pub fn server_default_ssh_key(app: AppHandle) -> Option<String> {
    let home = app.path().home_dir().ok()?;
    ssh_key::default_key(&home).map(|p| p.display().to_string())
}

/// Removes a host no lab runs on (see `server::setup::remove`).
#[tauri::command]
pub async fn server_remove(app: AppHandle, id: String) -> Result<()> {
    setup::remove(&AppDirs::from(&app), &id)
}

/// Sets (or clears, with None) the host VM labs run on by default.
#[tauri::command]
pub fn server_set_default(app: AppHandle, id: Option<String>) -> Result<()> {
    update(&AppDirs::from(&app), |store| set_default_host(store, id))
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
    Ok(crate::runtime::ssh::ensure_key(&AppDirs::from(&app)).await?.1)
}

/// How many installed labs are currently running on each host, keyed by host id (each lab's
/// host marker).
#[tauri::command]
pub fn server_running_labs(app: AppHandle) -> HashMap<String, u32> {
    running_by_host(&AppDirs::from(&app))
}

/// A host's hardware headroom, for its row: `None` for a host that isn't a Proxmox server, an
/// error when it couldn't be read (the row then shows none).
#[tauri::command]
pub async fn server_capacity(app: AppHandle, id: String) -> Result<Option<HostCapacity>> {
    capacity(&AppDirs::from(&app), &id).await
}

/// This month's spend so far (USD) on the saved AWS account `id`, with its own credentials (see
/// `server::budget::account_month_to_date_cost`).
#[tauri::command]
pub async fn server_month_to_date_cost(app: AppHandle, id: String) -> Result<f64> {
    account_month_to_date_cost(&AppDirs::from(&app), &id).await
}

/// Runs a real-VM self-test against host `id`, streaming step events (see
/// [`crate::runtime::server_selftest::run`]).
#[tauri::command]
pub async fn server_selftest(app: AppHandle, id: String, events: Channel<Event>) -> Result<()> {
    crate::runtime::server_selftest::run(&AppDirs::from(&app), &id, move |e| {
        let _ = events.send(e);
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::providers::Provider;
    use crate::runtime::server::HOST_MARKER;

    #[tokio::test]
    async fn hosts_are_saved_listed_tested_and_removed_through_the_commands() {
        use crate::platform::exec::fake::{Tools, ok};
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
        let tools = Tools::rules(&[("account show", ok("Pay-As-You-Go\n"))]);
        let result = server_test(app(), saved.id.clone()).await.unwrap();
        assert!(tools.called("az account show --subscription 00000000-0000-0000-0000-000000000000"));
        assert!(!result.checks.is_empty());
        assert!(server_capacity(app(), saved.id.clone()).await.unwrap().is_none(), "only Proxmox reports capacity");
        assert_eq!(server_month_to_date_cost(app(), saved.id.clone()).await.unwrap_err().to_string(), "only AWS accounts report their spend");
        // A lab running on it keeps it.
        let lab = t.lab("web", &[]);
        std::fs::write(lab.join(HOST_MARKER), &saved.id).unwrap();
        assert_eq!(server_running_labs(app()).get(&saved.id), Some(&1));
        assert!(server_remove(app(), saved.id.clone()).await.unwrap_err().to_string().starts_with("1 lab still running"));
        std::fs::remove_file(lab.join(HOST_MARKER)).unwrap();
        server_remove(app(), saved.id.clone()).await.unwrap();
        assert_eq!(server_list(app()).unwrap().hosts, Vec::<HostProfile>::new());
        // The launcher's key, made once.
        let ssh = t.data().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "k").unwrap();
        std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher\n").unwrap();
        assert_eq!(server_public_key(app()).await.unwrap(), "ssh-ed25519 AAAA cyberctf-launcher");
        // The default key offered is one of the usual ones, when the player has one.
        if let Some(path) = server_default_ssh_key(app()) {
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
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
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

        // AWS is billed: never a test. A folder a previous run left is cleared first.
        t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
        let work = t.data().join("selftest").join("server").join("aws1");
        std::fs::create_dir_all(&work).unwrap();
        let events = crate::testing::test_app::Sink::default();
        let err = server_selftest(app(), "aws1".into(), events.channel()).await.unwrap_err();
        assert!(err.to_string().contains("AWS labs are billed"), "{err}");
        assert!(steps(&events).contains(&pair("prepare", "ok")), "{:?}", steps(&events));
        assert!(!work.exists());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }
}
