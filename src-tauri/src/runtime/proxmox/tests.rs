use super::*;
use serde_json::json;

#[test]
fn recognises_token_ids() {
    assert!(is_token("root@pam!cyberctf"));
    assert!(!is_token("root@pam"));
    assert!(!is_token("root!x"));
    assert!(!is_token("root@pam!"));
    assert_eq!(token_user("root@pam!cyberctf"), "root@pam");
    assert_eq!(ssh_user("root@pam!cyberctf"), "root");
    assert_eq!(ssh_user("admin@pve"), "admin");
    assert_eq!(terraform_token("root@pam!cyberctf", "abc"), "root@pam!cyberctf=abc");
}

fn host(addr: &str, network: Option<&str>) -> HostProfile {
    HostProfile {
        gcp_project: None,
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
fn internal_bridges_have_no_ports() {
    let ifaces = json!([{"iface": "vmbr0", "bridge_ports": "eno1"}, {"iface": "vmbr1", "bridge_ports": " "}, {"iface": "vmbr2"}]);
    assert!(!find_iface(&ifaces, "vmbr0").is_some_and(has_no_ports));
    assert!(find_iface(&ifaces, "vmbr1").is_some_and(has_no_ports));
    assert!(find_iface(&ifaces, "vmbr2").is_some_and(has_no_ports));
    assert!(find_iface(&ifaces, "vmbr9").is_none());
    assert!(find_iface(&json!(null), "vmbr0").is_none());
}

#[test]
fn storage_problems_name_the_fix() {
    let ok = json!([{"storage": "local", "content": "iso,vztmpl,snippets,backup"}, {"storage": "local-lvm", "content": "images,rootdir"}]);
    assert!(storage_problems(&ok, "local-lvm").is_empty());
    let stock = json!([{"storage": "local", "content": "iso,vztmpl,backup"}, {"storage": "local-lvm", "content": "images,rootdir"}]);
    let p = storage_problems(&stock, "local-lvm");
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("Snippets"));
    let p = storage_problems(&stock, "fast-ssd");
    assert!(p.iter().any(|m| m.contains("\"fast-ssd\" doesn't exist")));
}

/// "Test connection" against a real host (opt-in):
///   CYBERCTF_TEST_PVE_HOST=... CYBERCTF_TEST_PVE_PASSWORD=... [CYBERCTF_TEST_PVE_TOKEN_ID=user@realm!name CYBERCTF_TEST_PVE_TOKEN_SECRET=...
///   CYBERCTF_TEST_SSH_KEY=<key authorized on the node>] cargo test proxmox_live_test -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn proxmox_live_test() {
    let var = |k: &str| std::env::var(k).ok();
    let address = var("CYBERCTF_TEST_PVE_HOST").expect("CYBERCTF_TEST_PVE_HOST");
    let profile = |username: &str, bridge: &str| HostProfile {
        username: username.into(),
        datastore: Some(var("CYBERCTF_TEST_PVE_STORAGE").unwrap_or_else(|| "local-lvm".into())),
        node: Some("pve".into()),
        insecure_tls: true,
        ..host(&address, Some(bridge))
    };
    let bridge = var("CYBERCTF_TEST_PVE_BRIDGE").unwrap_or_else(|| "vmbr1".into());
    let password = var("CYBERCTF_TEST_PVE_PASSWORD").expect("CYBERCTF_TEST_PVE_PASSWORD");

    let r = test(&profile("root@pam", &bridge), &password).await;
    println!("password: ok={} {}", r.ok, r.message);
    assert!(r.ok);
    let r = test(&profile("root@pam", &bridge), "wrong").await;
    println!("bad password: {}", r.message);
    assert_eq!(r.authenticated, Some(false));
    let r = test(&profile("root@pam", "vmbr-nope"), &password).await;
    println!("bad bridge: {}", r.message);
    assert!(!r.ok && r.message.contains("vmbr-nope"));

    if let (Some(secret), Some(key)) = (var("CYBERCTF_TEST_PVE_TOKEN_SECRET"), var("CYBERCTF_TEST_SSH_KEY")) {
        ssh::set_launcher_key_for_test(key.into());
        let token = var("CYBERCTF_TEST_PVE_TOKEN_ID").unwrap_or_else(|| "root@pam!cyberctf-live".into());
        let r = test(&profile(&token, &bridge), &secret).await;
        println!("token: ok={} {}", r.ok, r.message);
        assert!(r.ok);
        let r = test(&profile(&token, &bridge), "00000000-0000-0000-0000-000000000000").await;
        println!("bad token: {}", r.message);
        assert_eq!(r.authenticated, Some(false));
    }
}

#[test]
fn the_configured_node_or_the_first() {
    let nodes = json!([{"node": "pve"}, {"node": "pve2"}]);
    assert_eq!(pick_node(&nodes, None).unwrap(), "pve");
    assert_eq!(pick_node(&nodes, Some("")).unwrap(), "pve");
    assert_eq!(pick_node(&nodes, Some("pve2")).unwrap(), "pve2");
    let err = pick_node(&nodes, Some("nope")).unwrap_err().to_string();
    assert_eq!(err, "there is no node \"nope\" on this host (found: pve, pve2)");
    assert_eq!(pick_node(&json!([]), None).unwrap_err().to_string(), "this user can't see any node");
    assert!(pick_node(&json!(null), None).is_err());
}

#[test]
fn call_errors_read_as_http_codes_or_their_message() {
    assert_eq!(CallError::Status(401).to_string(), "HTTP 401");
    assert_eq!(CallError::Other("boom".into()).to_string(), "boom");
}

#[test]
fn sign_in_failures_say_what_to_fix() {
    let r = sign_in_report(SignInError::Rejected, true);
    assert!(!r.ok && r.authenticated == Some(false) && r.message.contains("API token"));
    let r = sign_in_report(SignInError::Rejected, false);
    assert!(r.authenticated == Some(false) && r.message.contains("root@pam"));
    let r = sign_in_report(SignInError::Failed("TLS".into()), false);
    assert_eq!(r.authenticated, None);
    assert_eq!(r.message, "Reachable, but the API call failed: TLS");
}

#[test]
fn node_listing_failures_are_signed_in_failures() {
    let r = node_report(CallError::Status(403));
    assert!(!r.ok && r.authenticated == Some(true) && r.message.contains("Administrator role"));
    let r = node_report(CallError::Status(500));
    assert_eq!(r.message, "Signed in, but listing nodes failed: HTTP 500");
}

#[test]
fn a_missing_bridge_is_a_problem() {
    let ifaces = json!([{"iface": "vmbr0"}]);
    assert_eq!(bridge_problem(&ifaces, "vmbr0", "pve"), None);
    assert!(bridge_problem(&ifaces, "vmbr1", "pve").unwrap().starts_with("Bridge \"vmbr1\" doesn't exist on pve."));
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

#[test]
fn storage_needs_each_content_once() {
    // The VM disks on `local` too: still one check per (storage, content).
    let all = json!([{"storage": "local", "content": "iso, snippets, images"}]);
    assert!(storage_problems(&all, "local").is_empty());
    assert_eq!(storage_content(&json!([{"storage": "x"}]), "x"), Some(vec![String::new()]));
    assert_eq!(storage_content(&json!({}), "x"), None);
}

#[test]
fn authorizing_the_key_signs_in_by_password_once() {
    let dir = std::env::temp_dir().join(format!("cyberctf-pve-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let identity = dir.join("id_ed25519");
    let askpass = write_askpass(&identity).unwrap();
    assert_eq!(askpass, dir.join("askpass.sh"));
    assert!(std::fs::read_to_string(&askpass).unwrap().contains("$CYBERCTF_SSH_PASSWORD"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&askpass).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let args = authorize_args(&host("pve.lan", None), &identity, "ssh-ed25519 AAAA launcher\n");
    assert_eq!(args[..4], ["-o", "PreferredAuthentications=password,keyboard-interactive", "-o", "NumberOfPasswordPrompts=1"]);
    // Escaped for ssh's option parser (a Windows path's backslashes, a space).
    assert!(args.contains(&format!("UserKnownHostsFile={}", ssh::ssh_config_path(&dir.join("known_hosts").display().to_string()))));
    assert_eq!(args[args.len() - 3..args.len() - 1], ["--", "root@pve.lan"]);
    assert!(args.last().unwrap().contains("grep -qxF 'ssh-ed25519 AAAA launcher' ~/.ssh/authorized_keys"), "{args:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn an_unreachable_host_fails_to_sign_in() {
    // Port 1 on the loopback refuses at once: no network beyond this machine.
    let mut h = host("127.0.0.1", None);
    h.port = 1;
    assert!(matches!(sign_in(&h, "pw").await, Err(SignInError::Failed(_))));
    let r = test(&h, "pw").await;
    assert!(!r.ok && r.authenticated.is_none(), "{}", r.message);
    assert!(!bridge_is_internal(&h, "pw").await);
    h.username = "root@pam!t".into();
    assert!(matches!(sign_in(&h, "pw").await, Err(SignInError::Failed(_))));
}
