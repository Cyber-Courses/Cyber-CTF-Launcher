use super::*;
use serde_json::json;

/// `json`, a list of what a path answers, read.
fn listed<T: serde::de::DeserializeOwned>(json: serde_json::Value) -> Vec<T> {
    serde_json::from_value(json).unwrap()
}

#[test]
fn a_login_without_a_realm_is_on_pam() {
    assert_eq!(with_default_realm("root"), "root@pam");
    assert_eq!(with_default_realm("admin@pve"), "admin@pve");
    assert_eq!(with_default_realm("root@pam"), "root@pam");
    assert_eq!(with_default_realm("root!ci"), "root@pam!ci");
    assert_eq!(with_default_realm("admin@pve!ci"), "admin@pve!ci");
    assert_eq!(with_default_realm(""), "");
}

#[test]
fn recognises_token_ids() {
    let pattern = regex::Regex::new(TOKEN_PATTERN).unwrap();
    for login in ["root@pam!cyberctf", "root@pam", "root!x", "root@pam!", "api@pve!ci!2", "@!x", "root!x@pam"] {
        assert_eq!(pattern.is_match(login), is_token(login), "the setup form's pattern reads {login} as Isothrum does");
    }
    assert!(is_token("root@pam!cyberctf"));
    assert!(!is_token("root@pam"));
    assert!(!is_token("root!x"));
    assert!(!is_token("root@pam!"));
    assert!(is_token("api@pve!ci!2") && is_token("@!x") && !is_token("root!x@pam"), "the realm comes before the first `!`");
    assert_eq!(token_user("root@pam!cyberctf"), "root@pam");
    assert_eq!(ssh_user("root@pam!cyberctf"), "root");
    assert_eq!(ssh_user("admin@pve"), "admin");
    assert_eq!(terraform_token("root@pam!cyberctf", "abc"), "root@pam!cyberctf=abc");
}

fn host(addr: &str, network: Option<&str>) -> HostProfile {
    HostProfile {
        gcp_project: None,
        ssh_key: None,
        image_storage: None,
        snippet_storage: None,
        tls_fingerprint: None,
        id: "t".into(),
        name: "t".into(),
        provider: crate::runtime::providers::Provider::Proxmox,
        host: addr.into(),
        port: 8006,
        username: "root@pam".into(),
        datastore: None,
        network: network.map(str::to_string),
        node: None,
        insecure_tls: false,
        auto_stop_hours: None,
        use_cli_creds: false,
        aws_profile: None,
        monthly_limit: None,
    }
}

#[test]
fn urls_bracket_ipv6_hosts() {
    assert_eq!(api_url(&host("10.0.0.5", None)), "https://10.0.0.5:8006/api2/json");
    assert_eq!(api_url(&host("fd00::5", None)), "https://[fd00::5]:8006/api2/json");
    assert_eq!(terraform_endpoint(&host("pve.lan", None)), "https://pve.lan:8006/");
    assert_eq!(terraform_endpoint(&host("fd00::5", None)), "https://[fd00::5]:8006/");
    assert_eq!(node_login(&host("pve.lan", None)), "root@pve.lan");
}

#[test]
fn lab_bridge_defaults_to_vmbr0() {
    assert_eq!(lab_bridge(&host("h", None)), "vmbr0");
    assert_eq!(lab_bridge(&host("h", Some(""))), "vmbr0");
    assert_eq!(lab_bridge(&host("h", Some("vmbr1"))), "vmbr1");
}

#[test]
fn storage_problems_name_the_fix() {
    let ok: Vec<api::Storage> =
        listed(json!([{"storage": "local", "content": "iso,vztmpl,snippets,backup"}, {"storage": "local-lvm", "content": "images,rootdir"}]));
    assert_eq!(storage_problems(&ok, "local-lvm", IMAGE_STORAGE, SNIPPET_STORAGE), Vec::<String>::new());
    let stock: Vec<api::Storage> = listed(json!([{"storage": "local", "content": "iso,vztmpl,backup"}, {"storage": "local-lvm", "content": "images,rootdir"}]));
    let p = storage_problems(&stock, "local-lvm", IMAGE_STORAGE, SNIPPET_STORAGE);
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("Snippets"));
    let p = storage_problems(&stock, "fast-ssd", IMAGE_STORAGE, SNIPPET_STORAGE);
    assert!(p.iter().any(|m| m.contains("\"fast-ssd\" doesn't exist")));
}

#[test]
fn sign_in_failures_say_what_to_fix() {
    let token = HostProfile { username: "root@pam!t".into(), ..host("h", None) };
    let r = sign_in_report(SignInError::Rejected, &token);
    assert!(!r.ok && r.authenticated == Some(false) && r.message.contains("API token"));
    let r = sign_in_report(SignInError::Rejected, &host("h", None));
    assert!(r.authenticated == Some(false) && r.message.contains("root@pam"));
    let r = sign_in_report(SignInError::Failed("TLS".into()), &host("h", None));
    assert_eq!(r.authenticated, None);
    assert_eq!(r.message, "Reachable, but the API call failed: TLS");
}

#[test]
fn node_listing_failures_are_signed_in_failures() {
    let r = node_report(CallError::Status(403));
    assert!(!r.ok && r.authenticated == Some(true) && r.message.contains("Administrator role"));
    let r = node_report(CallError::Status(500));
    assert_eq!(r.message, "Signed in, but listing nodes failed: HTTP 500");
    let r = node_report(worded(CallError::NoSuchNode { asked: "nope".into(), found: vec!["pve".into(), "pve2".into()] }));
    assert_eq!(r.message, "Signed in, but listing nodes failed: there is no node \"nope\" on this host (found: pve, pve2)");
}

#[test]
fn a_missing_bridge_is_a_problem() {
    let missing = isothrum_machine::place::proxmox::check::Finding::NoBridge { bridge: "vmbr1".into() };
    assert!(check::problem(&missing, "pve").starts_with("Bridge \"vmbr1\" doesn't exist on pve."));
}

#[test]
fn the_outcome_lists_every_problem() {
    let ok = outcome("pve", "vmbr0", &[]);
    assert!(ok.ok && ok.authenticated == Some(true));
    assert!(ok.message.contains("node pve") && ok.message.contains("vmbr0"));
    let bad = outcome("pve", "vmbr0", &["one".into(), "two".into()]);
    assert!(!bad.ok);
    assert_eq!(bad.message, "Signed in, but a lab launch would fail:\n• one\n• two");
}

#[tokio::test]
async fn an_unreachable_host_fails_to_sign_in() {
    // Port 1 on the loopback refuses at once: no network beyond this machine.
    let mut h = host("127.0.0.1", None);
    h.port = 1;
    assert!(matches!(sign_in(&h, "pw").await, Err(SignInError::Failed(_))));
    // Fails at sign-in, before the key is ever used.
    let r = test(&h, "pw", std::path::Path::new("/nonexistent/id_ed25519")).await;
    assert!(!r.ok && r.authenticated.is_none(), "{}", r.message);
    // Not knowing whether the bridge is internal fails the launch: guessing "no" hung Terraform.
    let err = bridge_is_internal(&h, "pw").await.unwrap_err().to_string();
    assert!(err.starts_with("Couldn't check how this machine reaches the Proxmox lab network ("), "{err}");
    h.username = "root@pam!t".into();
    assert!(matches!(sign_in(&h, "pw").await, Err(SignInError::Failed(_))));
}

#[test]
fn a_missing_sdn_directive_is_reported_with_its_fix() {
    use isothrum_machine::place::ssh::SshError;
    let missing = sdn_outcome(Ok(false), None).unwrap();
    assert_eq!(missing, SDN_MISSING);
    assert!(missing.contains("source /etc/network/interfaces.d/*") && missing.contains("ifreload -a"), "{missing}");
    assert_eq!(sdn_outcome(Ok(true), Some("root@pve".into())), None);
    // SSH itself failing: a token setup's SSH problem is reported elsewhere; a password one's here.
    let refused = || Err(SshError::Failed("Permission denied".into()));
    assert_eq!(sdn_outcome(refused(), None), None);
    assert!(
        sdn_outcome(refused(), Some("root@pve.lan".into())).unwrap().starts_with("Couldn't check the node's network configuration over SSH as root@pve.lan")
    );
}

#[tokio::test]
async fn the_sdn_check_runs_over_ssh_in_both_sign_in_modes() {
    use crate::testing::tools::{Tools, fail, ok};
    let app = crate::testing::test_app::TestApp::new();
    let key = app.data().join("ssh").join("id_ed25519");
    std::fs::create_dir_all(key.parent().unwrap()).unwrap();
    // A token setup: the launcher's key, as the token's user.
    let token = HostProfile { username: "root@pam!cyberctf-0a1b2c3d".into(), ..host("10.0.0.50", None) };
    let tools = Tools::strict_rules(&[("/etc/network/interfaces", fail(""))]);
    assert_eq!(sdn_problem(&token, "secret", &key).await.as_deref(), Some(SDN_MISSING));
    let call = tools.calls().pop().unwrap();
    assert!(call.starts_with("ssh -i ") && call.contains(" root@10.0.0.50 ") && call.ends_with("/etc/network/interfaces"), "{call}");
    drop(tools);
    let tools = Tools::strict_rules(&[("/etc/network/interfaces", ok(""))]);
    assert_eq!(sdn_problem(&token, "secret", &key).await, None);
    drop(tools);
    // A password setup: the password, through the askpass helper.
    let tools = Tools::strict_rules(&[("/etc/network/interfaces", fail(""))]);
    assert_eq!(sdn_problem(&host("10.0.0.50", None), "pw", &key).await.as_deref(), Some(SDN_MISSING));
    assert_eq!(tools.env_of("/etc/network/interfaces", "ISOTHRUM_SSH_PASSWORD").as_deref(), Some("pw"));
    assert!(tools.calls().pop().unwrap().contains("PreferredAuthentications=password"));
}

/// Regression: the launcher's key path was process-wide, pinned by whichever test made a key
/// first, so a later check wrote its askpass helper into that test's app folder after it was
/// removed: one stray folder per test run in the real Application Support. The helper goes
/// beside the key of the app it is given, and nowhere else.
#[tokio::test]
async fn the_sdn_check_writes_its_askpass_in_the_given_apps_folder_only() {
    use crate::testing::tools::{Tools, fail};
    let (first, second) = (crate::testing::test_app::TestApp::new(), crate::testing::test_app::TestApp::new());
    let _tools = Tools::strict_rules(&[("/etc/network/interfaces", fail(""))]);
    for app in [&first, &second] {
        let key = app.data().join("ssh").join("id_ed25519");
        std::fs::create_dir_all(key.parent().unwrap()).unwrap();
        sdn_problem(&host("10.0.0.50", None), "pw", &key).await;
    }
    let askpass = if cfg!(windows) { "askpass.cmd" } else { "askpass.sh" };
    for app in [&first, &second] {
        assert!(app.data().join("ssh").join(askpass).is_file(), "{}", app.data().display());
    }
    // The first app gone, a check for the second leaves nothing where the first one was.
    let gone = first.data().to_path_buf();
    drop(first);
    sdn_problem(&host("10.0.0.50", None), "pw", &second.data().join("ssh").join("id_ed25519")).await;
    assert!(!gone.exists(), "{} came back", gone.display());
}

#[tokio::test]
async fn a_self_signed_host_gets_no_credential_until_its_certificate_is_confirmed() {
    use crate::runtime::server::tls_pin::{
        Fingerprint, Refusal,
        tests::{FINGERPRINT_A, server_a},
    };
    let (port, requests) = server_a().await;
    let mut h = HostProfile { port, insecure_tls: true, ..host("127.0.0.1", None) };
    match sign_in(&h, "pw").await {
        Err(SignInError::Certificate(Refusal::Unconfirmed { presented }, server)) => {
            assert_eq!((presented.to_string(), server.as_str()), (FINGERPRINT_A.to_owned(), "t"));
        }
        other => panic!("{:?}", other.err()),
    }
    // Test connection says it, with its fingerprint, and a launch's error keeps its code.
    let r = test(&h, "pw", std::path::Path::new("/nonexistent/id_ed25519")).await;
    assert!(!r.ok && r.message.contains(FINGERPRINT_A), "{}", r.message);
    let e = bridge_is_internal(&h, "pw").await.unwrap_err();
    assert!(matches!(e.code(), crate::error::Code::TlsUnconfirmed { .. }), "{e}");
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0, "the password never left");
    // Confirmed: the sign-in reaches the server (whose canned answer isn't Proxmox's).
    h.tls_fingerprint = Fingerprint::parse(FINGERPRINT_A);
    assert!(matches!(sign_in(&h, "pw").await, Err(SignInError::Failed(_))));
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
}
