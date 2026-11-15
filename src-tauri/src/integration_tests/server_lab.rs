//! Real servers through the launcher's own code: a Proxmox host's connection test, and VM labs
//! through the runner path, end to end. `tests/integration/run.sh`'s server tier runs them
//! (`pnpm test:servers`), each skipped, naming what is missing, without its settings.
//!
//! The connection test (`a_proxmox_host_passes_the_connection_test`) is the Servers page's Test:
//! root's password, a wrong password and a bridge that isn't there; then, given an API token and a
//! key the node lets in, the token and a wrong token secret.
//!
//! ```text
//! CYBERCTF_TEST_PVE_HOST=<address> CYBERCTF_TEST_PVE_PASSWORD=<root's password> \
//! [CYBERCTF_TEST_PVE_TOKEN_ID='<user@realm!name>' CYBERCTF_TEST_PVE_TOKEN_SECRET=<its secret> \
//!  CYBERCTF_TEST_PVE_SSH_KEY=<a private key the node lets in> CYBERCTF_TEST_PVE_FINGERPRINT=<its certificate's SHA-256>] \
//! cargo test --lib -- --ignored --nocapture --exact integration_tests::server_lab::a_proxmox_host_passes_the_connection_test
//! ```
//!
//! A lab on Proxmox: the
//! lab generated for `proxmox`, started with the server's profile variables (the node passed in,
//! as the launcher discovers it), its status, its checks over SSH through the lab's router (and
//! the node, when the lab bridge is internal to it), then stopped, nothing of it left in the
//! runner's records. The same `isothrum::ServerRun` the lab page uses, without the app handle.
//!
//! Opt-in (it needs a Proxmox server, the runner's key let in on its node, and a lab):
//!
//! ```text
//! CYBERCTF_TEST_PVE_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_PVE_ENDPOINT=https://<address>:8006/ \
//! CYBERCTF_TEST_PVE_TOKEN_ID='<user@realm!name>' CYBERCTF_TEST_PVE_TOKEN_SECRET=<its secret> \
//! CYBERCTF_TEST_PVE_NODE=<node> CYBERCTF_TEST_PVE_BRIDGE=<bridge> CYBERCTF_TEST_PVE_JUMP=root@<address> \
//! CYBERCTF_TEST_PVE_RUNNER_HOME=<a folder whose ssh/id_ed25519 the node lets in> \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 --exact integration_tests::server_lab::a_vm_lab_runs_on_proxmox_through_the_runner
//! ```
//!
//! On ESXi (Vagrant's `vmware_esxi`, the runner relaying the host's SSH and HTTPS from macOS):
//! the lab generated for `vagrant`, started with the server's settings as the launcher keeps them
//! (`lab::esxi_env`) and its password as the run's connection, its status (its entry at the VM's
//! own address, fetched with Apple's curl, which macOS lets reach the LAN), its checks, its
//! provisioning again, then stopped, no VM of it left. Opt-in (it needs an ESXi host and a lab):
//!
//! ```text
//! CYBERCTF_TEST_ESXI_LAB=<isoloom checkout>/examples/solo-web CYBERCTF_TEST_ESXI_HOST=<address> \
//! CYBERCTF_TEST_ESXI_PASSWORD=… CYBERCTF_TEST_ESXI_DATASTORE=<datastore> CYBERCTF_TEST_ESXI_NETWORK='<port group>' \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 --exact integration_tests::server_lab::a_vm_lab_runs_on_esxi_through_the_runner
//! ```

use std::path::PathBuf;

use super::support::{check, scenario};
use crate::runtime::Runtime;
use crate::runtime::isothrum::{EsxiPlace, LabRun, ServerPlace, ServerRun};
use crate::runtime::lab;
use crate::runtime::server::HostProfile;

/// Knob `k`, which the test can't run without.
fn var(k: &str) -> String {
    super::support::var(k).unwrap_or_else(|| panic!("{k} is required"))
}

/// A fresh copy of the lab the knob `knob` names, as `name` in the temp folder, without its
/// generated files.
fn copy_lab(knob: &str, name: &str) -> PathBuf {
    let src = PathBuf::from(var(knob));
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::process::Command::new("cp").arg("-R").arg(&src).arg(&dir).status().unwrap().success());
    let _ = std::fs::remove_dir_all(dir.join(".isoloom"));
    dir
}

#[tokio::test]
#[ignore = "needs a Proxmox server; see the module docs"]
async fn a_proxmox_host_passes_the_connection_test() {
    use crate::runtime::proxmox::test;
    let (Some(address), Some(password)) = (super::support::knob("CYBERCTF_TEST_PVE_HOST"), super::support::knob("CYBERCTF_TEST_PVE_PASSWORD")) else {
        return;
    };
    let opt = super::support::var;
    let profile = |username: &str, bridge: &str| HostProfile {
        gcp_project: None,
        ssh_key: None,
        image_storage: None,
        snippet_storage: None,
        // Its certificate, as `pvenode cert info` shows it: a self-signed one is refused without.
        tls_fingerprint: opt("CYBERCTF_TEST_PVE_FINGERPRINT").and_then(|f| crate::runtime::server::tls_pin::Fingerprint::parse(&f)),
        id: "t".into(),
        name: "t".into(),
        provider: crate::runtime::providers::Provider::Proxmox,
        host: address.clone(),
        port: 8006,
        username: username.into(),
        datastore: Some(opt("CYBERCTF_TEST_PVE_STORAGE").unwrap_or_else(|| "local-lvm".into())),
        network: Some(bridge.into()),
        node: Some(opt("CYBERCTF_TEST_PVE_NODE").unwrap_or_else(|| "pve".into())),
        insecure_tls: true,
        auto_stop_hours: None,
        use_cli_creds: false,
        aws_profile: None,
        monthly_limit: None,
    };
    let bridge = opt("CYBERCTF_TEST_PVE_BRIDGE").unwrap_or_else(|| "vmbr1".into());
    // The launcher's key in the scenario's app folder: a password setup keeps its askpass beside it.
    let app = crate::testing::test_app::TestApp::scenario();
    let (key, _) = crate::runtime::ssh::ensure_key(&app.dirs()).await.unwrap();
    let mut f = Vec::new();

    let r = test(&profile("root@pam", &bridge), &password, &key).await;
    check(r.ok, format!("root's password passes ({})", r.message), &mut f);
    let r = test(&profile("root@pam", &bridge), "wrong", &key).await;
    check(r.authenticated == Some(false), format!("a wrong password is refused ({})", r.message), &mut f);
    let r = test(&profile("root@pam", "vmbr-nope"), &password, &key).await;
    check(!r.ok && r.message.contains("vmbr-nope"), format!("a bridge that isn't there is named ({})", r.message), &mut f);

    if let (Some(token), Some(secret), Some(ssh_key)) =
        (opt("CYBERCTF_TEST_PVE_TOKEN_ID"), opt("CYBERCTF_TEST_PVE_TOKEN_SECRET"), opt("CYBERCTF_TEST_PVE_SSH_KEY"))
    {
        let ssh_key = PathBuf::from(ssh_key);
        let r = test(&profile(&token, &bridge), &secret, &ssh_key).await;
        check(r.ok, format!("the API token passes ({})", r.message), &mut f);
        let r = test(&profile(&token, &bridge), "00000000-0000-0000-0000-000000000000", &ssh_key).await;
        check(r.authenticated == Some(false), format!("a wrong token secret is refused ({})", r.message), &mut f);
    } else {
        println!("the token checks left out: CYBERCTF_TEST_PVE_TOKEN_ID, _TOKEN_SECRET and _SSH_KEY aren't all set");
    }
    assert!(f.is_empty(), "{f:#?}");
}

#[tokio::test]
#[ignore = "needs a Proxmox server; see the module docs"]
async fn a_vm_lab_runs_on_proxmox_through_the_runner() {
    let dir = copy_lab("CYBERCTF_TEST_PVE_LAB", "cyberctf-pve-runner-lab");
    lab::prepare(&dir, lab::Target::Proxmox).unwrap();
    let endpoint = var("CYBERCTF_TEST_PVE_ENDPOINT");
    let host = endpoint.trim_start_matches("https://").split(':').next().unwrap().to_owned();
    let vars = [
        ("proxmox_endpoint", endpoint.clone()),
        ("proxmox_username", "root@pam".into()),
        ("proxmox_insecure", "true".into()),
        ("proxmox_ssh_address", host),
        ("proxmox_api_token", crate::runtime::proxmox::terraform_token(&var("CYBERCTF_TEST_PVE_TOKEN_ID"), &var("CYBERCTF_TEST_PVE_TOKEN_SECRET"))),
        ("proxmox_ssh_username", "root".into()),
        ("uplink_bridge", var("CYBERCTF_TEST_PVE_BRIDGE")),
    ];
    let place = ServerPlace {
        label: "pve-lab".into(),
        name: "Proxmox test server".into(),
        vars: vars.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        // The profile names no node: the launcher passes the server's first.
        node: Some(var("CYBERCTF_TEST_PVE_NODE")),
        jump: std::env::var("CYBERCTF_TEST_PVE_JUMP").ok().filter(|j| !j.is_empty()),
    };
    let home = PathBuf::from(var("CYBERCTF_TEST_PVE_RUNNER_HOME"));
    let run = ServerRun::new(&home, &dir, "pve-runner-lab", Runtime::Vm).unwrap();
    let mut f = Vec::new();
    scenario(
        &mut f,
        async |f: &mut Vec<String>| {
            let up = run.up(&place, &[], &mut log("pve")).await;
            check(up.is_ok(), format!("it starts ({:?})", up.err()), f);
            match run.status(&place).await {
                Ok(s) => {
                    println!("status: running={} url={:?} machines={:?}", s.running, s.url, s.machines.iter().map(|m| (&m.name, &m.state)).collect::<Vec<_>>());
                    check(s.running && s.url.is_some(), format!("it runs, with its entry (running={} url={:?})", s.running, s.url), f);
                }
                Err(e) => check(false, format!("its status reads: {e}"), f),
            }
            checks_pass(Ok(run.check()), f);
        },
        async |f: &mut Vec<String>| {
            let down = run.down(&place, &mut log("pve")).await;
            check(down.is_ok(), format!("it stops ({:?})", down.err()), f);
            check(run.status(&place).await.is_ok_and(|s| !s.running), "its status reads as gone".into(), f);
        },
    )
    .await;
    assert!(f.is_empty(), "{f:#?}");
}

/// The test's log line, tagged with the server's kind.
fn log(tag: &'static str) -> impl FnMut(String) {
    move |l| println!("[{tag}] {l}")
}

/// The lab's checks, read through the runner, all pass.
fn checks_pass(checked: crate::error::Result<crate::runtime::docker::Check>, f: &mut Vec<String>) {
    match checked {
        Ok(c) => {
            println!("checks: ok={} {:?}", c.ok, c.results.iter().map(|r| (&r.name, r.ok)).collect::<Vec<_>>());
            check(c.ok, format!("its checks pass ({})", c.output.trim()), f);
        }
        Err(e) => check(false, format!("its checks run: {e}"), f),
    }
}

/// The HTTP status `url` answers with, fetched with Apple's curl (which macOS lets reach the LAN).
fn curl(url: &str) -> std::result::Result<String, String> {
    let out = std::process::Command::new("/usr/bin/curl")
        .args(["-s", "-m", "15", "-o", "/dev/null", "-w", "%{http_code}", url])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[tokio::test]
#[ignore = "needs an ESXi host; see the module docs"]
async fn a_vm_lab_runs_on_esxi_through_the_runner() {
    let dir = copy_lab("CYBERCTF_TEST_ESXI_LAB", "cyberctf-esxi-runner-lab");
    lab::prepare(&dir, lab::Target::Vagrant).unwrap();
    let profile: HostProfile = serde_json::from_value(serde_json::json!({
        "id": "esxi-lab",
        "name": "ESXi test server",
        "provider": "vmware_esxi",
        "host": var("CYBERCTF_TEST_ESXI_HOST"),
        "port": 22,
        "username": "root",
        "datastore": std::env::var("CYBERCTF_TEST_ESXI_DATASTORE").ok(),
        "network": std::env::var("CYBERCTF_TEST_ESXI_NETWORK").ok(),
    }))
    .unwrap();
    let env = lab::esxi_env(&profile, &var("CYBERCTF_TEST_ESXI_PASSWORD"));
    let home = std::env::temp_dir().join("cyberctf-esxi-runner-home");
    let run = LabRun::on_esxi(&home, &dir, "esxi-runner-lab", Runtime::Vm, EsxiPlace::from_env(&profile.id, &profile.name, &env)).unwrap();
    let mut f = Vec::new();
    scenario(
        &mut f,
        async |f: &mut Vec<String>| {
            let up = run.up(crate::runtime::launch::Inputs::default(), Some("vmware_esxi"), &mut log("esxi")).await;
            check(up.is_ok(), format!("it starts ({:?})", up.err()), f);
            match run.status().await {
                Ok(s) => {
                    println!("status: running={} url={:?} machines={:?}", s.running, s.url, s.machines.iter().map(|m| (&m.name, &m.state)).collect::<Vec<_>>());
                    match s.url {
                        // Its entry at the VM's own address, fetched while it runs.
                        Some(url) => {
                            check(s.running && !url.contains("127.0.0.1"), format!("it runs, at the VM's own address ({url})"), f);
                            let code = curl(&url);
                            check(code.as_deref() == Ok("200"), format!("{url} answers ({code:?})"), f);
                        }
                        None => check(false, "it has its entry".into(), f),
                    }
                }
                Err(e) => check(false, format!("its status reads: {e}"), f),
            }
            checks_pass(run.check().await, f);
            let provisioned = run.provision(None, &mut log("esxi")).await;
            check(provisioned.is_ok(), format!("it provisions again ({:?})", provisioned.err()), f);
        },
        async |f: &mut Vec<String>| {
            let down = run.down(&mut log("esxi")).await;
            check(down.is_ok(), format!("it stops ({:?})", down.err()), f);
            check(run.status().await.is_ok_and(|s| !s.running), "its status reads as gone".into(), f);
        },
    )
    .await;
    assert!(f.is_empty(), "{f:#?}");
}
