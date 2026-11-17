use super::*;
use crate::testing::tools::{Tools, ok};

/// Two public host keys (made for these tests), with the fingerprints `ssh-keygen -lf` gives them.
const KEY_1: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIMV/X9GBl8lRmDMg/hSFR41QZH/fgA5PLNatzgxigSSL";
const FINGERPRINT_1: &str = "SHA256:Coh2bRLeV1HsLRUtSn+G8PFvq/E3/excrj4hN1k1YEQ";
const KEY_2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAICqe9BTRKrDHvwjUyYLAuhUCKs3XEHQ043EztdSCVfwq";

/// `node`'s key line, as a scan and `known_hosts` write it.
fn line(node: &str, key: &str) -> String {
    format!("{node} ssh-ed25519 {key}\n")
}

/// A node offering `offered`, its keys confirmed in the file `KEY_1` when `confirmed` (else no
/// file yet), and the folder that file is in.
fn node(offered: &str, confirmed: bool) -> (Tools, crate::testing::TempDir, PathBuf) {
    let dir = crate::testing::temp_dir("node-keys");
    let file = dir.join("nodes_known_hosts");
    if confirmed {
        std::fs::write(&file, line("10.0.0.5", KEY_1)).unwrap();
    }
    // `ssh-keygen -F` reads the file only once it is written: always with `KEY_1` then.
    let tools = Tools::strict_rules(&[
        ("ssh-keyscan -T 10 -p 22 -t ed25519,ecdsa,rsa 10.0.0.5", ok(line("10.0.0.5", offered))),
        ("ssh-keygen -F 10.0.0.5", ok(line("10.0.0.5", KEY_1))),
    ]);
    (tools, dir, file)
}

#[tokio::test]
async fn a_node_whose_keys_are_confirmed_asks_nothing() {
    let (tools, _dir, file) = node(KEY_1, true);
    assert_eq!(unconfirmed_at("10.0.0.5", &file).await.unwrap(), None, "no question: the run goes on");
    confirm_at("10.0.0.5", &[FINGERPRINT_1.into()], &file).await.unwrap();
    assert!(!tools.called("ssh-keygen -R"), "nothing rewritten: {:?}", tools.calls());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), line("10.0.0.5", KEY_1));
}

#[tokio::test]
async fn a_node_never_confirmed_is_shown_then_confirmed_once_for_every_run() {
    let (_tools, _dir, file) = node(KEY_1, false);
    let shown = unconfirmed_at("10.0.0.5", &file).await.unwrap().expect("the player is asked");
    assert_eq!(shown.state, known_hosts::State::New);
    assert_eq!(shown.keys.iter().map(|k| k.fingerprint.as_str()).collect::<Vec<_>>(), [FINGERPRINT_1]);
    confirm_at("10.0.0.5", &[FINGERPRINT_1.into()], &file).await.unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), line("10.0.0.5", KEY_1), "the run's jump checks it there");
    // Read again (the next run): nothing asked.
    assert_eq!(unconfirmed_at("10.0.0.5", &file).await.unwrap(), None);
}

#[tokio::test]
async fn a_node_never_confirmed_whose_keys_are_declined_stays_unconfirmed() {
    let (_tools, _dir, file) = node(KEY_1, false);
    assert!(unconfirmed_at("10.0.0.5", &file).await.unwrap().is_some());
    // Declined: nothing confirmed, so the next run asks again and the jump trusts nothing.
    assert!(!file.exists(), "nothing written");
    assert!(unconfirmed_at("10.0.0.5", &file).await.unwrap().is_some());
}

#[tokio::test]
async fn a_node_offering_other_keys_than_the_confirmed_ones_is_refused_and_never_replaced() {
    let (tools, _dir, file) = node(KEY_2, true);
    let refused = unconfirmed_at("10.0.0.5", &file).await.unwrap_err();
    assert_eq!(refused.code(), Code::SshHostKeyRefused { host: "10.0.0.5".into() }, "the host key changed wording");
    let refused = confirm_at("10.0.0.5", &[FINGERPRINT_1.into()], &file).await.unwrap_err();
    assert_eq!(refused.code(), Code::SshHostKeyRefused { host: "10.0.0.5".into() });
    assert!(!tools.called("ssh-keygen -R"), "{:?}", tools.calls());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), line("10.0.0.5", KEY_1), "the confirmed keys kept");
}

#[tokio::test]
async fn a_server_whose_labs_dont_go_through_its_node_asks_nothing() {
    let t = crate::testing::test_app::TestApp::new();
    let esxi = t.host("esxi1", "vmware_esxi", serde_json::json!({}));
    let tools = Tools::strict_rules(&[]);
    assert_eq!(unconfirmed(&t.dirs(), &esxi).await.unwrap(), None);
    assert!(confirm(&t.dirs(), &esxi, &[FINGERPRINT_1.into()]).await.is_err(), "no node keys to confirm");
    assert!(tools.calls().is_empty(), "the node never reached: {:?}", tools.calls());
}
