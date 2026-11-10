use std::path::Path;

use super::*;
use crate::runtime::server::HOST_MARKER;
use crate::runtime::server::hosts::tests::{host, store};

/// A Proxmox form signing in with the SSH key at `key`.
fn ssh_key_input(key: &Path) -> HostInput {
    serde_json::from_value(serde_json::json!({
        "id": null, "name": "Garage", "provider": "proxmox", "host": "10.0.0.50", "port": 8006, "username": "",
        "datastore": null, "network": null, "node": null, "insecureTls": true, "autoStopHours": null, "password": null,
        "sshKey": { "path": key.display().to_string(), "user": "root", "port": 22 }
    }))
    .unwrap()
}

/// An Azure account's form.
fn azure(id: Option<&str>, region: &str, subscription: &str, name: &str) -> HostInput {
    serde_json::from_value(
        serde_json::json!({ "id": id, "name": name, "provider": "azure", "host": region, "username": subscription, "datastore": "Standard_B2s" }),
    )
    .unwrap()
}

/// A Proxmox server's form signing in as root, its password `password` (none: keep the stored one).
fn proxmox(id: Option<&str>, password: Option<&str>) -> HostInput {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": "P", "provider": "proxmox", "host": "127.0.0.1", "port": 1, "username": "root",
        "datastore": null, "network": null, "node": null, "insecureTls": true, "autoStopHours": null, "password": password
    }))
    .unwrap()
}

const SUBSCRIPTION: &str = "00000000-0000-0000-0000-000000000000";

#[test]
fn a_form_saves_a_new_host_then_edits_it_and_an_unknown_one_is_refused() {
    let t = crate::testing::test_app::TestApp::new();
    let saved = save(&t.dirs(), azure(None, "westeurope", SUBSCRIPTION, "Azure")).unwrap();
    assert_eq!(save(&t.dirs(), azure(Some(&saved.id), "westeurope", SUBSCRIPTION, "Renamed")).unwrap().id, saved.id);
    assert_eq!(load(&t.dirs()).unwrap().hosts.len(), 1);
    assert_eq!(save(&t.dirs(), azure(Some("ffffffffffffffff"), "westeurope", SUBSCRIPTION, "Azure")).unwrap_err().to_string(), "unknown server host");
}

#[tokio::test]
async fn placement_choices_need_a_proxmox_server_and_its_secret() {
    let t = crate::testing::test_app::TestApp::new();
    let err = |r: Result<placement::Placement>| r.unwrap_err().to_string();
    let esxi: HostInput = serde_json::from_value(
        serde_json::json!({ "id": null, "name": "E", "provider": "vmware_esxi", "host": "127.0.0.1", "username": "root", "password": "pw" }),
    )
    .unwrap();
    assert_eq!(err(placement(&t.dirs(), esxi).await), "only Proxmox servers list their placement");
    assert_eq!(err(placement(&t.dirs(), proxmox(None, None)).await), "enter the password or token secret first");
    // Nothing listens on port 1: the read fails and says so (the setup then offers manual entry).
    assert!(err(placement(&t.dirs(), proxmox(None, Some("pw"))).await).starts_with("Couldn't read the server's nodes, storages and bridges"));
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn an_ssh_key_setup_saves_the_token_it_made_like_a_token_setup() {
    use crate::platform::exec::fake::{Tools, fail, ok};
    let t = crate::testing::test_app::TestApp::new();
    crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
    // The launcher's key, already made; the player's own key (never read, only its path used).
    let ssh = t.data().join("ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("id_ed25519"), "k").unwrap();
    std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher\n").unwrap();
    let mine = t.data().join("my_key");
    std::fs::write(&mine, "PRIVATE KEY CONTENTS").unwrap();

    // The node refuses the key: nothing is saved, and the message says which part failed.
    let tools = Tools::rules(&[("authorized_keys", fail("root@10.0.0.50: Permission denied (publickey)."))]);
    let err = save_with_ssh_key(&t.dirs(), ssh_key_input(&mine)).await.unwrap_err().to_string();
    assert!(err.starts_with("Couldn't sign in to the node over SSH with your key"), "{err}");
    assert!(!tools.called("pveum"), "no token is made when SSH fails");
    assert!(load(&t.dirs()).unwrap().hosts.is_empty());
    drop(tools);

    let name = ssh_key::token_name("ssh-ed25519 AAAA cyberctf-launcher");
    let answer = format!(r#"{{"full-tokenid":"root@pam!{name}","info":{{"privsep":"0"}},"value":"6f1d2c3b-aaaa-bbbb-cccc-0123456789ab"}}"#);
    let tools = Tools::rules(&[("authorized_keys", ok("added")), ("pveum user token", ok(answer))]);
    let saved = save_with_ssh_key(&t.dirs(), ssh_key_input(&mine)).await.unwrap();
    assert_eq!(saved.username, format!("root@pam!{name}"), "a token setup from here on");
    assert!(crate::runtime::proxmox::is_token(&saved.username));
    assert_eq!(saved.ssh_key.as_ref().map(|k| k.path.clone()), Some(mine.display().to_string()));
    assert_eq!(get_secret(&saved.id).unwrap(), "6f1d2c3b-aaaa-bbbb-cccc-0123456789ab", "the token's secret is in the keychain");
    assert_eq!(tools.calls().len(), 2);
    assert!(tools.calls().iter().all(|c| !c.contains("PRIVATE KEY CONTENTS")), "the key's contents are never passed on");
    assert!(made_with_key(&saved.id).is_some_and(|m| m.contains(&name) && m.contains("added to root's authorized_keys")), "kept for its Test connection");
    drop(tools);

    // Saving it again (its placement, say) with the same key and token keeps the key.
    let again: HostInput = serde_json::from_value(serde_json::json!({
        "id": saved.id, "name": "Garage", "provider": "proxmox", "host": "10.0.0.50", "port": 8006, "username": saved.username,
        "datastore": "local-lvm", "network": "vmbr1", "node": "pve", "insecureTls": true, "autoStopHours": null, "password": null,
        "sshKey": { "path": mine.display().to_string(), "user": "root", "port": 22 }, "imageStorage": "local", "snippetStorage": "local"
    }))
    .unwrap();
    let kept = save(&t.dirs(), again).unwrap();
    assert_eq!(kept.ssh_key, saved.ssh_key);
    assert_eq!((kept.network.as_deref(), kept.image_storage.as_deref()), (Some("vmbr1"), Some("local")));
    assert_eq!(get_secret(&saved.id).unwrap(), "6f1d2c3b-aaaa-bbbb-cccc-0123456789ab", "the token's secret is kept");
    // A password save of the same host drops the SSH key sign-in.
    let password: HostInput = serde_json::from_value(serde_json::json!({
        "id": saved.id, "name": "Garage", "provider": "proxmox", "host": "10.0.0.50", "port": 8006, "username": "root@pam",
        "datastore": null, "network": null, "node": null, "insecureTls": true, "autoStopHours": null, "password": "pw",
        "sshKey": { "path": mine.display().to_string(), "user": "root", "port": 22 }
    }))
    .unwrap();
    let resaved = save(&t.dirs(), password).unwrap();
    assert_eq!((resaved.ssh_key, resaved.username.as_str()), (None, "root@pam"));
    assert_eq!(get_secret(&saved.id).unwrap(), "pw");
}

#[cfg(debug_assertions)]
#[test]
fn a_blank_password_keeps_the_stored_one_asks_for_a_missing_one_and_says_why_one_cant_be_read() {
    let t = crate::testing::test_app::TestApp::new();
    let file = t.data().join("secrets.json");
    crate::runtime::server::secrets::use_test_file(file.clone());
    assert_eq!(save(&t.dirs(), proxmox(None, None)).unwrap_err().to_string(), "enter the host's password");
    let saved = save(&t.dirs(), proxmox(None, Some("pw"))).unwrap();
    save(&t.dirs(), proxmox(Some(&saved.id), None)).unwrap();
    assert_eq!(get_secret(&saved.id).unwrap(), "pw", "kept");
    // Regression: a store that couldn't be read asked for the password again, as if none was kept.
    std::fs::write(&file, "not json").unwrap();
    let err = save(&t.dirs(), proxmox(Some(&saved.id), None)).unwrap_err().to_string();
    assert!(err.starts_with("the secrets file") && err.contains("isn't readable"), "{err}");
}

#[test]
fn an_account_with_labs_on_it_keeps_its_place_but_not_its_credentials() {
    // Regression: the player moved an AWS account with a lab running in eu-west-3 to
    // us-east-1 for their next lab; Stop then destroyed in us-east-1, found nothing, and the
    // instance kept billing in eu-west-3.
    let t = crate::testing::test_app::TestApp::new();
    let saved = save(&t.dirs(), azure(None, "westeurope", SUBSCRIPTION, "Azure")).unwrap();
    let lab = t.lab("placed", &[]);
    std::fs::write(lab.join(HOST_MARKER), &saved.id).unwrap();
    let err = save(&t.dirs(), azure(Some(&saved.id), "eastus", SUBSCRIPTION, "Azure")).unwrap_err().to_string();
    assert_eq!(err, "1 lab is still on this account: stop it before changing its address or region.");
    let err = save(&t.dirs(), azure(Some(&saved.id), "westeurope", "11111111-0000-0000-0000-000000000000", "Azure")).unwrap_err().to_string();
    assert!(err.ends_with("changing its account."), "{err}");
    assert_eq!(load(&t.dirs()).unwrap().hosts[0].host, "westeurope", "nothing saved");
    // Its name (and credentials, size, auto-stop) can change.
    assert_eq!(save(&t.dirs(), azure(Some(&saved.id), "westeurope", SUBSCRIPTION, "Renamed")).unwrap().name, "Renamed");
    // Stopped: it can move.
    std::fs::remove_file(lab.join(HOST_MARKER)).unwrap();
    assert_eq!(save(&t.dirs(), azure(Some(&saved.id), "eastus", SUBSCRIPTION, "Renamed")).unwrap().host, "eastus");
}

#[test]
fn saves_edit_known_hosts_or_make_new_ones() {
    let s = store(vec![host("ab12", Provider::Proxmox)], None);
    assert_eq!(save_id(&s, Some("ab12".into())).unwrap(), "ab12");
    assert_eq!(save_id(&s, Some("cd34".into())).unwrap_err().to_string(), "unknown server host");
    assert_eq!(save_id(&s, Some("../x".into())).unwrap_err().to_string(), "unknown server host");
    let fresh = save_id(&s, None).unwrap();
    assert_eq!(fresh.len(), 16);
    assert_ne!(fresh, "ab12");
}

#[test]
fn gcp_keeps_its_project_unless_the_billing_account_changed() {
    let gcp = HostProfile { username: "AAAAAA-BBBBBB-CCCCCC".into(), gcp_project: Some("cyberctf-labs-abc123".into()), ..host("ab12", Provider::Gcp) };
    let s = store(vec![gcp], None);
    assert_eq!(kept_gcp_project(&s, "ab12", Provider::Gcp, "AAAAAA-BBBBBB-CCCCCC").as_deref(), Some("cyberctf-labs-abc123"));
    assert_eq!(kept_gcp_project(&s, "ab12", Provider::Gcp, "DDDDDD-BBBBBB-CCCCCC"), None);
    assert_eq!(kept_gcp_project(&s, "ab12", Provider::Aws, "AAAAAA-BBBBBB-CCCCCC"), None);
    assert_eq!(kept_gcp_project(&s, "zz", Provider::Gcp, "AAAAAA-BBBBBB-CCCCCC"), None);
}

#[test]
fn passwords_are_kept_when_blank_and_bounded_when_given() {
    assert_eq!(new_password(None).unwrap(), None);
    assert_eq!(new_password(Some(String::new())).unwrap(), None);
    assert_eq!(new_password(Some("hunter2".into())).unwrap().as_deref(), Some("hunter2"));
    assert!(new_password(Some("a\0b".into())).is_err());
    assert!(new_password(Some("x".repeat(1025))).is_err());
    assert!(new_password(Some("x".repeat(1024))).is_ok());
}

#[test]
fn a_deployment_left_by_an_uninstalled_lab_keeps_its_host() {
    // The lab's folder is gone (uninstalled) but its Terraform state still holds resources
    // on the account: only this host's credentials can destroy them, so it stays.
    let t = crate::testing::test_app::TestApp::new();
    let host = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    let state = t.data().join("deployments").join("web").join("aws");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{},"resources":[{"type":"aws_instance"}]}"#).unwrap();
    std::fs::write(state.join("host"), "aws1\n").unwrap();
    let err = remove(&t.dirs(), &host).unwrap_err().to_string();
    assert_eq!(err, "1 lab still running on this host. Stop it before removing the host.");
    assert_eq!(load(&t.dirs()).unwrap().hosts.len(), 1);
    assert!(crate::runtime::server::hosts::running_by_host(&t.dirs()).is_empty(), "no installed lab is marked as running there");
    // Destroyed (its state gone): free to remove.
    std::fs::remove_file(state.join("terraform.tfstate")).unwrap();
    remove(&t.dirs(), &host).unwrap();
    assert!(load(&t.dirs()).unwrap().hosts.is_empty());
}

#[test]
fn removing_a_host_that_is_not_saved_changes_nothing() {
    let t = crate::testing::test_app::TestApp::new();
    t.host("az1", "azure", serde_json::json!({}));
    remove(&t.dirs(), "nope").unwrap();
    assert_eq!(load(&t.dirs()).unwrap().hosts.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["az1"]);
    // A marker of a lab on a host that was never saved blocks nothing else.
    let lab = t.lab("web", &[]);
    std::fs::write(lab.join(HOST_MARKER), "nope").unwrap();
    assert_eq!(crate::runtime::server::hosts::running_by_host(&t.dirs()).get("nope"), Some(&1));
    remove(&t.dirs(), "az1").unwrap();
    assert!(load(&t.dirs()).unwrap().hosts.is_empty());
}

#[cfg(debug_assertions)]
#[test]
fn a_host_whose_secret_cant_be_removed_stays_and_says_why() {
    // Regression: the secret's removal failed unseen, the host went, and its password stayed.
    let t = crate::testing::test_app::TestApp::new();
    let file = t.data().join("secrets.json");
    crate::runtime::server::secrets::use_test_file(file.clone());
    let host = t.host("pve1", "proxmox", serde_json::json!({}));
    // A folder where the secrets file should be: it can't be read, let alone changed.
    std::fs::create_dir_all(file.join("x")).unwrap();
    let err = remove(&t.dirs(), &host).unwrap_err().to_string();
    assert!(err.starts_with("couldn't remove this host's secret, so the host stays"), "{err}");
    assert_eq!(load(&t.dirs()).unwrap().hosts.len(), 1);
    std::fs::remove_dir_all(&file).unwrap();
    set_secret(&host, "pw").unwrap();
    remove(&t.dirs(), &host).unwrap();
    assert!(load(&t.dirs()).unwrap().hosts.is_empty());
    assert_eq!(get_secret(&host).unwrap_err().code(), Code::NoPasswordStored, "its secret went with it");
}
