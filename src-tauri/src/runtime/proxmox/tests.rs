use super::*;
use serde_json::json;

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
    assert_eq!(storage_problems(&ok, "local-lvm", IMAGE_STORAGE, SNIPPET_STORAGE), Vec::<String>::new());
    let stock = json!([{"storage": "local", "content": "iso,vztmpl,backup"}, {"storage": "local-lvm", "content": "images,rootdir"}]);
    let p = storage_problems(&stock, "local-lvm", IMAGE_STORAGE, SNIPPET_STORAGE);
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("Snippets"));
    let p = storage_problems(&stock, "fast-ssd", IMAGE_STORAGE, SNIPPET_STORAGE);
    assert!(p.iter().any(|m| m.contains("\"fast-ssd\" doesn't exist")));
}

/// "Test connection" against a real host (opt-in):
///   `CYBERCTF_TEST_PVE_HOST`=... `CYBERCTF_TEST_PVE_PASSWORD`=... [`CYBERCTF_TEST_PVE_TOKEN_ID=user@realm!name` `CYBERCTF_TEST_PVE_TOKEN_SECRET`=...
///   `CYBERCTF_TEST_SSH_KEY`=<key authorized on the node>] cargo test `proxmox_live_test` -- --ignored --nocapture
#[tokio::test]
#[ignore = "opt-in: connects to a real Proxmox host set via CYBERCTF_TEST_PVE_* env vars"]
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
    // The launcher's key in a throwaway app folder: a password setup keeps its askpass beside it.
    let app = crate::testing::test_app::TestApp::new();
    let (key, _) = ssh::ensure_key(&app.dirs()).await.unwrap();

    let r = test(&profile("root@pam", &bridge), &password, &key).await;
    println!("password: ok={} {}", r.ok, r.message);
    assert!(r.ok);
    let r = test(&profile("root@pam", &bridge), "wrong", &key).await;
    println!("bad password: {}", r.message);
    assert_eq!(r.authenticated, Some(false));
    let r = test(&profile("root@pam", "vmbr-nope"), &password, &key).await;
    println!("bad bridge: {}", r.message);
    assert!(!r.ok && r.message.contains("vmbr-nope"));

    if let (Some(secret), Some(key)) = (var("CYBERCTF_TEST_PVE_TOKEN_SECRET"), var("CYBERCTF_TEST_SSH_KEY")) {
        let key = std::path::PathBuf::from(key);
        let token = var("CYBERCTF_TEST_PVE_TOKEN_ID").unwrap_or_else(|| "root@pam!cyberctf-live".into());
        let r = test(&profile(&token, &bridge), &secret, &key).await;
        println!("token: ok={} {}", r.ok, r.message);
        assert!(r.ok);
        let r = test(&profile(&token, &bridge), "00000000-0000-0000-0000-000000000000", &key).await;
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
    assert_eq!(storage_problems(&all, "local", IMAGE_STORAGE, SNIPPET_STORAGE), Vec::<String>::new());
    assert_eq!(storage_content(&json!([{"storage": "x"}]), "x"), Some(vec![String::new()]));
    assert_eq!(storage_content(&json!({}), "x"), None);
}

#[test]
fn authorizing_the_key_signs_in_by_password_once() {
    let dir = std::env::temp_dir().join(format!("cyberctf-pve-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let identity = dir.join("id_ed25519");
    let askpass = write_askpass(&identity).unwrap();
    assert_eq!(askpass, dir.join(if cfg!(windows) { "askpass.cmd" } else { "askpass.sh" }));
    assert!(std::fs::read_to_string(&askpass).unwrap().contains("CYBERCTF_SSH_PASSWORD"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&askpass).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let args = authorize_args(&host("pve.lan", None), &identity, "ssh-ed25519 AAAA launcher\n");
    assert_eq!(args[..4], ["-o", "PreferredAuthentications=password,keyboard-interactive", "-o", "NumberOfPasswordPrompts=1"]);
    // A node that doesn't answer fails the connection in seconds, not at the OS's TCP timeout.
    assert_eq!(args[4..6], ["-o", "ConnectTimeout=10"]);
    // Escaped for ssh's option parser (a Windows path's backslashes, a space).
    assert!(args.contains(&format!("UserKnownHostsFile={}", ssh::ssh_config_path(&dir.join("nodes_known_hosts").display().to_string()))));
    assert_eq!(args[args.len() - 3..args.len() - 1], ["--", "root@pve.lan"]);
    assert!(args.last().unwrap().contains("grep -qxF 'ssh-ed25519 AAAA launcher' ~/.ssh/authorized_keys"), "{args:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Windows' OpenSSH runs the askpass as a program: a `sh` script there never answered, so a
/// password host behind an internal bridge couldn't launch from Windows.
#[test]
fn the_askpass_helper_is_a_cmd_on_windows_and_sh_elsewhere() {
    let (name, script) = askpass_script(true);
    assert_eq!(name, "askpass.cmd");
    assert!(script.starts_with("@echo off\r\n") && script.contains("EnableDelayedExpansion") && script.contains("echo(!CYBERCTF_SSH_PASSWORD!"));
    let (name, script) = askpass_script(false);
    assert_eq!(name, "askpass.sh");
    assert!(script.starts_with("#!/bin/sh\n"));
}

/// The sh helper prints the password exactly, whatever it holds.
#[cfg(unix)]
#[test]
fn the_sh_askpass_prints_the_password_as_is() {
    let dir = std::env::temp_dir().join(format!("cyberctf-askpass-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let askpass = write_askpass(&dir.join("id_ed25519")).unwrap();
    let password = "p@ss w$rd'&|%!\\n";
    let out = std::process::Command::new(&askpass).env("CYBERCTF_SSH_PASSWORD", password).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), format!("{password}\n"));
    std::fs::remove_dir_all(dir).unwrap();
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

/// The SDN check is a read-only grep that finds `source /etc/network/interfaces.d/*` (or `/sdn`)
/// and nothing else.
#[cfg(unix)]
#[test]
fn the_sdn_check_finds_only_a_real_source_directive() {
    let dir = std::env::temp_dir().join(format!("cyberctf-sdn-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("interfaces");
    let has_directive = |body: &str| {
        std::fs::write(&file, body).unwrap();
        let command = format!("{}{}", SDN_SOURCE_CHECK.strip_suffix("/etc/network/interfaces").unwrap(), file.display());
        std::process::Command::new("sh").args(["-c", &command]).status().unwrap().success()
    };
    let base = "auto lo\niface lo inet loopback\n\nauto vmbr0\niface vmbr0 inet static\n";
    assert!(has_directive(&format!("{base}\nsource /etc/network/interfaces.d/*\n")));
    assert!(has_directive(&format!("{base}  source  /etc/network/interfaces.d/sdn\n")));
    assert!(!has_directive(base), "the stock PVE file without the directive");
    assert!(!has_directive(&format!("{base}# source /etc/network/interfaces.d/*\n")), "commented out");
    assert!(!has_directive(&format!("{base}source /etc/network/interfaces.d/other\n")));
    assert!(SDN_SOURCE_CHECK.starts_with("grep -Eq ") && !SDN_SOURCE_CHECK.contains(';'), "one read-only command");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_missing_sdn_directive_is_reported_with_its_fix() {
    use crate::error::Error;
    let grep_found_nothing = || Err(Error::CommandFailed { command: "ssh".into(), stderr: String::new() });
    let missing = sdn_outcome(grep_found_nothing(), None).unwrap();
    assert_eq!(missing, SDN_MISSING);
    assert!(missing.contains("source /etc/network/interfaces.d/*") && missing.contains("ifreload -a"), "{missing}");
    assert_eq!(sdn_outcome(Ok(String::new()), Some("root@pve".into())), None);
    // SSH itself failing: a token setup's SSH problem is reported elsewhere; a password one's here.
    let refused = || Err(Error::CommandFailed { command: "ssh".into(), stderr: "Permission denied".into() });
    assert_eq!(sdn_outcome(refused(), None), None);
    assert!(
        sdn_outcome(refused(), Some("root@pve.lan".into())).unwrap().starts_with("Couldn't check the node's network configuration over SSH as root@pve.lan")
    );
}

#[tokio::test]
async fn the_sdn_check_runs_over_ssh_in_both_sign_in_modes() {
    use crate::platform::exec::fake::{Tools, fail, ok};
    let app = crate::testing::test_app::TestApp::new();
    let key = app.data().join("ssh").join("id_ed25519");
    std::fs::create_dir_all(key.parent().unwrap()).unwrap();
    // A token setup: the launcher's key, as the token's user.
    let token = HostProfile { username: "root@pam!cyberctf-0a1b2c3d".into(), ..host("10.0.0.50", None) };
    let tools = Tools::rules(&[("/etc/network/interfaces", fail(""))]);
    assert_eq!(sdn_problem(&token, "secret", &key).await.as_deref(), Some(SDN_MISSING));
    let call = tools.calls().pop().unwrap();
    assert!(call.starts_with("ssh -i ") && call.contains(" root@10.0.0.50 ") && call.ends_with(SDN_SOURCE_CHECK), "{call}");
    drop(tools);
    let tools = Tools::rules(&[("/etc/network/interfaces", ok(""))]);
    assert_eq!(sdn_problem(&token, "secret", &key).await, None);
    drop(tools);
    // A password setup: the password, through the askpass helper.
    let tools = Tools::rules(&[("/etc/network/interfaces", fail(""))]);
    assert_eq!(sdn_problem(&host("10.0.0.50", None), "pw", &key).await.as_deref(), Some(SDN_MISSING));
    assert_eq!(tools.env_of("/etc/network/interfaces", "CYBERCTF_SSH_PASSWORD").as_deref(), Some("pw"));
    assert!(tools.calls().pop().unwrap().contains("PreferredAuthentications=password"));
}

/// Regression: the launcher's key path was process-wide, pinned by whichever test made a key
/// first, so a later check wrote its askpass helper into that test's app folder after it was
/// removed: one stray folder per test run in the real Application Support. The helper goes
/// beside the key of the app it is given, and nowhere else.
#[tokio::test]
async fn the_sdn_check_writes_its_askpass_in_the_given_apps_folder_only() {
    use crate::platform::exec::fake::{Tools, fail};
    let (first, second) = (crate::testing::test_app::TestApp::new(), crate::testing::test_app::TestApp::new());
    let _tools = Tools::rules(&[("/etc/network/interfaces", fail(""))]);
    for app in [&first, &second] {
        let key = app.data().join("ssh").join("id_ed25519");
        std::fs::create_dir_all(key.parent().unwrap()).unwrap();
        sdn_problem(&host("10.0.0.50", None), "pw", &key).await;
    }
    let askpass = askpass_script(cfg!(windows)).0;
    for app in [&first, &second] {
        assert!(app.data().join("ssh").join(askpass).is_file(), "{}", app.data().display());
    }
    // The first app gone, a check for the second leaves nothing where the first one was.
    let gone = first.data().to_path_buf();
    drop(first);
    sdn_problem(&host("10.0.0.50", None), "pw", &second.data().join("ssh").join("id_ed25519")).await;
    assert!(!gone.exists(), "{} came back", gone.display());
}
