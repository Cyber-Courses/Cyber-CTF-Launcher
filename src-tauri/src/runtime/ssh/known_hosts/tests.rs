use super::*;
use crate::platform::exec::fake::{Tools, fail, ok};

/// Two public host keys (made for these tests), with the fingerprints `ssh-keygen -lf` gives them.
const KEY_1: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIMV/X9GBl8lRmDMg/hSFR41QZH/fgA5PLNatzgxigSSL";
const FINGERPRINT_1: &str = "SHA256:Coh2bRLeV1HsLRUtSn+G8PFvq/E3/excrj4hN1k1YEQ";
const KEY_2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAICqe9BTRKrDHvwjUyYLAuhUCKs3XEHQ043EztdSCVfwq";
const FINGERPRINT_2: &str = "SHA256:Z+fWYzq6ygA/L76meHUVttMwHeuEdgXt4uJu/8tVUEM";

fn scan_of(pattern: &str, key: &str) -> String {
    format!("{pattern} ssh-ed25519 {key}\n")
}

#[test]
fn a_key_reads_as_openssh_prints_its_fingerprint() {
    let key = HostKey { algorithm: "ssh-ed25519".into(), blob: KEY_1.into() };
    assert_eq!(key.fingerprint().as_deref(), Some(FINGERPRINT_1));
    assert_eq!(HostKey { algorithm: "x".into(), blob: "not base64!".into() }.fingerprint(), None);
    assert_eq!(pattern("10.0.0.50", 22), "10.0.0.50");
    assert_eq!(pattern("pve.lan", 2222), "[pve.lan]:2222");
}

#[test]
fn only_the_named_hosts_keys_are_read() {
    let text = format!(
        "# pve:22 SSH-2.0-OpenSSH_9.2\n{}other ssh-ed25519 {KEY_2}\n@revoked pve ssh-ed25519 {KEY_2}\n|1|c2FsdA==|aGFzaA== ssh-ed25519 {KEY_2}\n",
        scan_of("pve", KEY_1)
    );
    let keys = keys_in(&text, "pve");
    assert_eq!(keys.iter().map(|k| k.blob.as_str()).collect::<Vec<_>>(), [KEY_1, KEY_2], "the named one, and a hashed one ssh-keygen found");
}

#[test]
fn the_offered_keys_are_new_confirmed_or_changed() {
    let k1 = HostKey { algorithm: "ssh-ed25519".into(), blob: KEY_1.into() };
    let k2 = HostKey { algorithm: "ssh-ed25519".into(), blob: KEY_2.into() };
    assert_eq!(state(std::slice::from_ref(&k1), &[]), State::New);
    assert_eq!(state(&[k1.clone(), k2.clone()], std::slice::from_ref(&k1)), State::Confirmed);
    assert_eq!(state(std::slice::from_ref(&k2), std::slice::from_ref(&k1)), State::Changed);
}

#[tokio::test]
async fn a_new_node_is_shown_with_its_fingerprints_and_confirmed_into_every_file() {
    let dir = crate::testing::temp_dir("known-hosts");
    let files = [dir.join("a").join("nodes_known_hosts"), dir.join("b").join("nodes_known_hosts")];
    let tools = Tools::strict_rules(&[("ssh-keyscan", ok(scan_of("[pve.lan]:2222", KEY_1)))]);
    let offered = offered("pve.lan", 2222, &files[0]).await.unwrap();
    assert_eq!(offered.state, State::New);
    assert_eq!(offered.keys, [Shown { algorithm: "ssh-ed25519".into(), fingerprint: FINGERPRINT_1.into() }]);
    assert!(tools.called("ssh-keyscan -T 10 -p 2222 -t ed25519,ecdsa,rsa pve.lan"), "{:?}", tools.calls());
    confirm("pve.lan", 2222, &[FINGERPRINT_1.into()], &files).await.unwrap();
    for file in &files {
        assert_eq!(std::fs::read_to_string(file).unwrap(), format!("[pve.lan]:2222 ssh-ed25519 {KEY_1}\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}

#[tokio::test]
async fn keys_that_changed_after_they_were_shown_are_not_confirmed() {
    let dir = crate::testing::temp_dir("known-hosts-changed");
    let file = dir.join("nodes_known_hosts");
    let _tools = Tools::strict_rules(&[("ssh-keyscan", ok(scan_of("pve", KEY_2)))]);
    let e = confirm("pve", 22, &[FINGERPRINT_1.into()], std::slice::from_ref(&file)).await.unwrap_err();
    assert!(e.to_string().contains("changed while you were confirming"), "{e}");
    assert!(!file.exists(), "nothing written");
}

#[tokio::test]
async fn a_node_whose_keys_changed_says_so_and_a_new_confirmation_replaces_them() {
    let dir = crate::testing::temp_dir("known-hosts-repin");
    let file = dir.join("nodes_known_hosts");
    std::fs::write(&file, scan_of("pve", KEY_1)).unwrap();
    let tools =
        Tools::strict_rules(&[("ssh-keyscan", ok(scan_of("pve", KEY_2))), ("ssh-keygen -F pve", ok(scan_of("pve", KEY_1))), ("ssh-keygen -R pve", ok(""))]);
    let offered = offered("pve", 22, &file).await.unwrap();
    assert_eq!((offered.state, offered.keys[0].fingerprint.as_str()), (State::Changed, FINGERPRINT_2));
    confirm("pve", 22, &[FINGERPRINT_2.into()], std::slice::from_ref(&file)).await.unwrap();
    assert!(tools.called(&format!("ssh-keygen -R pve -f {}", file.display())), "the old key forgotten first: {:?}", tools.calls());
    assert!(std::fs::read_to_string(&file).unwrap().ends_with(&scan_of("pve", KEY_2)));
}

#[tokio::test]
async fn a_node_that_offers_no_key_or_cant_be_reached_says_so() {
    let dir = crate::testing::temp_dir("known-hosts-none");
    let file = dir.join("nodes_known_hosts");
    let _tools = Tools::strict_rules(&[
        ("ssh-keyscan -T 10 -p 22 -t ed25519,ecdsa,rsa silent", ok("")),
        ("ssh-keyscan -T 10 -p 22 -t ed25519,ecdsa,rsa down", fail("connect refused")),
    ]);
    assert!(offered("silent", 22, &file).await.unwrap_err().to_string().contains("offered no SSH host key"));
    assert!(offered("down", 22, &file).await.unwrap_err().to_string().contains("couldn't read the SSH host keys of down"));
    assert!(offered("-oProxyCommand=x", 22, &file).await.is_err(), "never an option");
}

#[test]
fn a_refused_host_key_is_the_learners_error() {
    let refused = Error::CommandFailed {
        command: "ssh".into(),
        stderr: "No ED25519 host key is known for pve and you have requested strict checking.\r\nHost key verification failed.".into(),
    };
    assert_eq!(refused_key("pve", refused).code(), crate::error::Code::SshHostKeyRefused { host: "pve".into() });
    let other = Error::CommandFailed { command: "ssh".into(), stderr: "Permission denied (publickey).".into() };
    assert!(matches!(refused_key("pve", other), Error::CommandFailed { .. }));
}
