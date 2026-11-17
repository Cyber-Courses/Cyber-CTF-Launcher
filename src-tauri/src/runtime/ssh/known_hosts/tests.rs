use super::*;
use isothrum_runner::fake::{FakeProcesses, Reply};

use crate::runtime::providers::testing::fake;

/// Two public host keys (made for these tests), with the fingerprints `ssh-keygen -lf` gives them.
const KEY_1: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIMV/X9GBl8lRmDMg/hSFR41QZH/fgA5PLNatzgxigSSL";
const FINGERPRINT_1: &str = "SHA256:Coh2bRLeV1HsLRUtSn+G8PFvq/E3/excrj4hN1k1YEQ";
const KEY_2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAICqe9BTRKrDHvwjUyYLAuhUCKs3XEHQ043EztdSCVfwq";
const FINGERPRINT_2: &str = "SHA256:Z+fWYzq6ygA/L76meHUVttMwHeuEdgXt4uJu/8tVUEM";

fn scan_of(pattern: &str, key: &str) -> String {
    format!("{pattern} ssh-ed25519 {key}\n")
}

#[tokio::test]
async fn a_new_node_is_shown_with_its_fingerprints_and_confirmed_into_every_file() {
    let dir = crate::testing::temp_dir("known-hosts");
    let files = [dir.join("a").join("nodes_known_hosts"), dir.join("b").join("nodes_known_hosts")];
    let tools = fake(FakeProcesses::script().expect("ssh-keyscan", Reply::ok(&scan_of("[pve.lan]:2222", KEY_1))).build());
    let offered = offered("pve.lan", 2222, &files[0]).await.unwrap();
    assert_eq!(offered.state, State::New);
    assert_eq!(offered.keys, [Shown { algorithm: "ssh-ed25519".into(), fingerprint: FINGERPRINT_1.into() }]);
    assert!(!tools.made("ssh-keyscan -T 10 -p 2222 -t ed25519,ecdsa,rsa pve.lan").is_empty(), "{:?}", tools.lines());
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
    let _tools = fake(FakeProcesses::script().expect("ssh-keyscan", Reply::ok(&scan_of("pve", KEY_2))).build());
    let e = confirm("pve", 22, &[FINGERPRINT_1.into()], std::slice::from_ref(&file)).await.unwrap_err();
    assert!(e.to_string().contains("changed while you were confirming"), "{e}");
    assert!(!file.exists(), "nothing written");
}

#[tokio::test]
async fn a_node_whose_keys_changed_says_so_and_a_new_confirmation_replaces_them() {
    let dir = crate::testing::temp_dir("known-hosts-repin");
    let file = dir.join("nodes_known_hosts");
    std::fs::write(&file, scan_of("pve", KEY_1)).unwrap();
    let tools = fake(
        FakeProcesses::script()
            .expect("ssh-keyscan", Reply::ok(&scan_of("pve", KEY_2)))
            .expect("ssh-keygen -F pve", Reply::ok(&scan_of("pve", KEY_1)))
            .expect("ssh-keygen -R pve", Reply::ok(""))
            .build(),
    );
    let offered = offered("pve", 22, &file).await.unwrap();
    assert_eq!((offered.state, offered.keys[0].fingerprint.as_str()), (State::Changed, FINGERPRINT_2));
    confirm("pve", 22, &[FINGERPRINT_2.into()], std::slice::from_ref(&file)).await.unwrap();
    let forgot = tools.made("ssh-keygen -R pve -f");
    assert_eq!(
        forgot.iter().map(|c| c.args.clone()).collect::<Vec<_>>(),
        [["-R", "pve", "-f", &file.display().to_string()]],
        "the old key forgotten first: {:?}",
        tools.lines()
    );
    assert!(std::fs::read_to_string(&file).unwrap().ends_with(&scan_of("pve", KEY_2)));
}

#[tokio::test]
async fn a_node_that_offers_no_key_or_cant_be_reached_says_so() {
    let dir = crate::testing::temp_dir("known-hosts-none");
    let file = dir.join("nodes_known_hosts");
    let _tools = fake(
        FakeProcesses::script()
            .expect("ssh-keyscan -T 10 -p 22 -t ed25519,ecdsa,rsa silent", Reply::ok(""))
            .expect("ssh-keyscan -T 10 -p 22 -t ed25519,ecdsa,rsa down", Reply::fail(1, "connect refused"))
            .build(),
    );
    assert!(offered("silent", 22, &file).await.unwrap_err().to_string().contains("offered no SSH host key"));
    assert!(offered("down", 22, &file).await.unwrap_err().to_string().contains("couldn't read the SSH host keys of down"));
    assert!(offered("-oProxyCommand=x", 22, &file).await.is_err(), "never an option");
}

#[test]
fn a_refused_host_key_is_the_learners_error() {
    let refused = super::super::ssh_error("pve", super::super::SshError::HostKeyRefused { host: "pve".into() });
    assert_eq!(refused.code(), crate::error::Code::SshHostKeyRefused { host: "pve".into() });
}
