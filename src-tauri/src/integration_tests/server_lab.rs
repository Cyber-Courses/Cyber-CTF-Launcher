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
//! as the launcher discovers it), its status with its entry where the module publishes it, its
//! link answering from this computer (a local forward through the node when only it reaches the
//! lab), a command run through its shell, its checks passing (run in its machines over SSH,
//! through the node first), then stopped, nothing of it left in the
//! runner's records. The same `isothrum::ServerRun` the lab page uses, without the app handle:
//! with `CYBERCTF_TEST_PVE_JUMP`, Terraform's SSH to the router goes through the launcher's
//! tunnel to the node, the node's host keys confirmed first as the player does in Servers.
//!
//! Opt-in (it needs a Proxmox server, the runner's key let in on its node, and a lab):
//!
//! ```text
//! CYBERCTF_TEST_PVE_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_PVE_ENDPOINT=https://<address>:8006/ \
//! CYBERCTF_TEST_PVE_TOKEN_ID='<user@realm!name>' CYBERCTF_TEST_PVE_TOKEN_SECRET=<its secret> \
//! CYBERCTF_TEST_PVE_NODE=<node> CYBERCTF_TEST_PVE_BRIDGE=<bridge> CYBERCTF_TEST_PVE_JUMP=root@<address> \
//! CYBERCTF_TEST_PVE_RUNNER_HOME=<a folder whose ssh/id_ed25519 the node lets in> \
//! CYBERCTF_TEST_PVE_SSH_KEY=<a private key the node lets root in with: the snippet upload's> \
//! [CYBERCTF_TEST_PVE_STORAGE=<its storage for VM disks, images and snippets; default local-lvm>] \
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

use std::path::{Path, PathBuf};

use super::support::{check, log, scenario};
use crate::runtime::Runtime;
use crate::runtime::isothrum::{EsxiPlace, LabRun, ServerPlace, ServerRun};
use crate::runtime::lab;
use crate::runtime::server::{HostProfile, SshKeySignIn};

/// Knob `k`, which the test can't run without.
fn var(k: &str) -> String {
    super::support::var(k).unwrap_or_else(|| panic!("{k} is required"))
}

/// A fresh copy of the lab the knob `knob` names, as `name` in the temp folder, without its
/// generated files, generated for `target`.
fn copy_lab(knob: &str, name: &str, target: lab::Target) -> PathBuf {
    let src = PathBuf::from(var(knob));
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    lab::copy_tree(&src, &dir).unwrap_or_else(|e| panic!("copy {} to {}: {e}", src.display(), dir.display()));
    let _ = std::fs::remove_dir_all(dir.join(".isoloom"));
    lab::prepare(&dir, target).unwrap_or_else(|e| panic!("the lab in {} generated for {target:?}: {e}", dir.display()));
    dir
}

/// Confirms the SSH host keys `address` offers, as the player does in Servers on first use, in the
/// node key file beside each of `identities` (`ssh::node_known_hosts`).
async fn trust_on_first_use(address: &str, identities: &[&Path]) {
    use crate::runtime::ssh::{known_hosts, node_known_hosts};
    let files: Vec<PathBuf> = identities.iter().map(|k| node_known_hosts(k)).collect();
    let offered = known_hosts::offered(address, 22, &files[0]).await.unwrap_or_else(|e| panic!("{address} offers its SSH host keys: {e}"));
    let shown: Vec<String> = offered.keys.into_iter().map(|k| k.fingerprint).collect();
    known_hosts::confirm(address, 22, &shown, &files).await.unwrap_or_else(|e| panic!("{address}'s SSH host keys are confirmed: {e}"));
}

/// The test server as Servers saves a Proxmox profile: at `address`:`port`, signed in as
/// `username`, its labs on `bridge`, its certificate, storage and node from the knobs.
fn proxmox_profile(address: &str, port: u16, username: &str, bridge: &str) -> HostProfile {
    let opt = super::support::var;
    HostProfile {
        gcp_project: None,
        ssh_key: None,
        image_storage: None,
        snippet_storage: None,
        // Its certificate, as `pvenode cert info` shows it: a self-signed one is refused without.
        tls_fingerprint: opt("CYBERCTF_TEST_PVE_FINGERPRINT").and_then(|f| crate::runtime::server::tls_pin::Fingerprint::parse(&f)),
        id: "t".into(),
        name: "t".into(),
        provider: crate::runtime::providers::Provider::Proxmox,
        host: address.to_owned(),
        port,
        username: username.into(),
        datastore: Some(opt("CYBERCTF_TEST_PVE_STORAGE").unwrap_or_else(|| "local-lvm".into())),
        network: Some(bridge.into()),
        node: Some(opt("CYBERCTF_TEST_PVE_NODE").unwrap_or_else(|| "pve".into())),
        insecure_tls: true,
        auto_stop_hours: None,
        use_cli_creds: false,
        aws_profile: None,
        monthly_limit: None,
    }
}

#[tokio::test]
#[ignore = "needs a Proxmox server; see the module docs"]
async fn a_proxmox_host_passes_the_connection_test() {
    use crate::runtime::proxmox::test;
    let (Some(address), Some(password)) = (super::support::knob("CYBERCTF_TEST_PVE_HOST"), super::support::knob("CYBERCTF_TEST_PVE_PASSWORD")) else {
        return;
    };
    let opt = super::support::var;
    let profile = |username: &str, bridge: &str| proxmox_profile(&address, 8006, username, bridge);
    let bridge = opt("CYBERCTF_TEST_PVE_BRIDGE").unwrap_or_else(|| "vmbr1".into());
    // The launcher's key in the scenario's app folder: a password setup keeps its askpass beside it.
    let app = crate::testing::test_app::TestApp::scenario();
    let (key, _) = crate::runtime::ssh::ensure_key(&app.dirs()).await.unwrap_or_else(|e| panic!("the launcher's SSH key is made: {e}"));
    let ssh_key = opt("CYBERCTF_TEST_PVE_SSH_KEY").map(PathBuf::from);
    // The node's SSH host keys, confirmed as the player confirms them on first use, beside each
    // key the test signs in with.
    let keys: Vec<&Path> = std::iter::once(key.as_path()).chain(ssh_key.as_deref()).collect();
    trust_on_first_use(&address, &keys).await;
    let mut f = Vec::new();

    let r = test(&profile("root@pam", &bridge), &password, &key).await;
    check(r.ok, format!("root's password passes ({})", r.message), &mut f);
    let r = test(&profile("root@pam", &bridge), "wrong", &key).await;
    check(r.authenticated == Some(false), format!("a wrong password is refused ({})", r.message), &mut f);
    let r = test(&profile("root@pam", "vmbr-nope"), &password, &key).await;
    // The message is the contract here: it is what the Servers page's Test shows the player, and
    // the result has no field of its own for the bridge.
    check(!r.ok && r.message.contains("vmbr-nope"), format!("a bridge that isn't there is named ({})", r.message), &mut f);

    if let (Some(token), Some(secret), Some(ssh_key)) = (opt("CYBERCTF_TEST_PVE_TOKEN_ID"), opt("CYBERCTF_TEST_PVE_TOKEN_SECRET"), ssh_key) {
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
    let dir = copy_lab("CYBERCTF_TEST_PVE_LAB", "cyberctf-pve-runner-lab", lab::Target::Proxmox);
    // The server's profile as a token setup with the node's key saves it, and its variables as a
    // launch makes them (`lab::host_vars`).
    let endpoint = var("CYBERCTF_TEST_PVE_ENDPOINT");
    let (address, port) = endpoint
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .split_once(':')
        .and_then(|(address, port)| Some((address, port.parse::<u16>().ok()?)))
        .unwrap_or_else(|| panic!("CYBERCTF_TEST_PVE_ENDPOINT: {endpoint} is no https://<address>:<port>/"));
    let storage = super::support::var("CYBERCTF_TEST_PVE_STORAGE");
    let profile = HostProfile {
        ssh_key: Some(SshKeySignIn { path: var("CYBERCTF_TEST_PVE_SSH_KEY"), user: "root".into(), port: 22 }),
        image_storage: storage.clone(),
        snippet_storage: storage,
        ..proxmox_profile(address, port, &var("CYBERCTF_TEST_PVE_TOKEN_ID"), &var("CYBERCTF_TEST_PVE_BRIDGE"))
    };
    let vars = lab::host_vars(&profile, &var("CYBERCTF_TEST_PVE_TOKEN_SECRET"));
    let home = PathBuf::from(var("CYBERCTF_TEST_PVE_RUNNER_HOME"));
    let run = ServerRun::new(&home, &dir, "pve-runner-lab", Runtime::Vm).unwrap_or_else(|e| panic!("the lab's run on the server opens: {e}"));
    // The key the lab's VMs let in, as a launch gives it (`launch::server_place`).
    let keys = run.key_values().await.unwrap_or_else(|e| panic!("the run's key for the lab's VMs reads: {e}"));
    let jump = std::env::var("CYBERCTF_TEST_PVE_JUMP").ok().filter(|j| !j.is_empty());
    if let Some((_, node)) = jump.as_deref().and_then(|j| j.split_once('@')) {
        // The tunnel checks the node's host key against the ones confirmed beside the key.
        trust_on_first_use(node, &[&home.join("ssh").join("id_ed25519")]).await;
    }
    let place = ServerPlace {
        label: "pve-lab".into(),
        name: "Proxmox test server".into(),
        vars: vars.into_iter().chain(keys).collect(),
        // The profile names no node: the launcher passes the server's first.
        node: Some(var("CYBERCTF_TEST_PVE_NODE")),
        jump,
    };
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
                    reached(&run, &s, f).await;
                }
                Err(e) => check(false, format!("its status reads: {e}"), f),
            }
            // Its checks run in its machines through the runner, over SSH through the router
            // (and through the node first when only it reaches the lab).
            checks_pass(run.check().await, f);
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

/// The lab of status `s` is reached from this computer: its link answers (at a local forward
/// through the node when only the node reaches the lab), and its shell reaches its machine
/// (through the node and the router).
async fn reached(run: &ServerRun, s: &crate::runtime::LabStatus, f: &mut Vec<String>) {
    // Where the runner says each published endpoint is reached, and why not when it isn't.
    match run.endpoints(&isothrum_runner::RunOptions::new()).await {
        Ok(endpoints) => {
            for e in &endpoints {
                println!("endpoint {}: {:?} (forwarded: {}) {}", e.name, e.address, e.forwarded, e.why.as_deref().unwrap_or_default());
            }
            check(endpoints.iter().all(|e| e.address.is_some()), format!("each published endpoint is reached ({endpoints:?})"), f);
        }
        Err(e) => check(false, format!("its endpoints read: {e}"), f),
    }
    if let Some(url) = &s.url {
        let code = curl(url);
        check(code.as_deref() == Ok("200"), format!("{url} answers ({code:?})"), f);
    }
    match s.machines.iter().find(|m| !m.infra) {
        Some(m) => shell_runs(run, &m.name, f).await,
        None => check(false, "it has a machine of its own".into(), f),
    }
}

/// A command runs in `machine` through its shell's command ("Open shell"'s, without a terminal).
async fn shell_runs(run: &ServerRun, machine: &str, f: &mut Vec<String>) {
    let shell = match run.console_command(machine, None).await {
        Ok(shell) => shell,
        Err(e) => return check(false, format!("{machine}'s shell opens: {e}"), f),
    };
    let ran = tokio::process::Command::new(&shell.program)
        .args(&shell.args)
        .arg("echo shell-reached-$(hostname)")
        .envs(shell.env.iter().map(|(k, v)| (k, v)))
        .current_dir(&shell.cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match ran {
        Ok(out) => {
            let said = String::from_utf8_lossy(&out.stdout);
            println!("shell: {}", said.trim());
            check(
                out.status.success() && said.contains("shell-reached-"),
                format!("{machine}'s shell runs a command ({}: {})", out.status, String::from_utf8_lossy(&out.stderr).trim()),
                f,
            );
        }
        Err(e) => check(false, format!("{machine}'s shell starts: {e}"), f),
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
    let dir = copy_lab("CYBERCTF_TEST_ESXI_LAB", "cyberctf-esxi-runner-lab", lab::Target::Vagrant);
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
    .unwrap_or_else(|e| panic!("the ESXi profile reads: {e}"));
    let env = lab::esxi_env(&profile, &var("CYBERCTF_TEST_ESXI_PASSWORD"));
    let home = std::env::temp_dir().join("cyberctf-esxi-runner-home");
    let run = LabRun::on_esxi(&home, &dir, "esxi-runner-lab", Runtime::Vm, EsxiPlace::from_env(&profile.id, &profile.name, &env))
        .unwrap_or_else(|e| panic!("the lab's run on ESXi opens: {e}"));
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
