//! Real VM labs on real servers through the launcher's runner path, end to end.
//!
//! On Proxmox: the
//! lab generated for `proxmox`, started with the server's profile variables (the node passed in,
//! as the launcher discovers it), its status, its checks over SSH through the lab's router (and
//! the node, when the lab bridge is internal to it), then stopped, nothing of it left in the
//! runner's records. The same `isothrum::ServerRun` the lab page uses, without the app handle.
//!
//! Opt-in (it needs a Proxmox server, the runner's key let in on its node, and a lab):
//!
//! ```text
//! CYBERCTF_TEST_PVE_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_PVE_ENDPOINT=https://172.16.1.10:8006/ CYBERCTF_TEST_PVE_TOKEN='root@pam!name=secret' \
//! CYBERCTF_TEST_PVE_NODE=pve-lab CYBERCTF_TEST_PVE_BRIDGE=vmbr1 CYBERCTF_TEST_PVE_JUMP=root@172.16.1.10 \
//! CYBERCTF_TEST_RUNNER_HOME=<a folder whose ssh/id_ed25519 the node lets in> \
//! cargo test real_server_labs -- --ignored --nocapture
//! ```
//!
//! On ESXi (Vagrant's `vmware_esxi`, the runner relaying the host's SSH and HTTPS from macOS):
//! the lab generated for `vagrant`, started with the server's settings as the launcher keeps them
//! (`lab::esxi_env`) and its password as the run's connection, its status (its entry at the VM's
//! own address, fetched with Apple's curl, which macOS lets reach the LAN), its checks, its
//! provisioning again, then stopped, no VM of it left. Opt-in (it needs an ESXi host and a lab):
//!
//! ```text
//! CYBERCTF_TEST_ESXI_LAB=<isoloom checkout>/examples/solo-web CYBERCTF_TEST_ESXI_HOST=172.16.1.20 \
//! CYBERCTF_TEST_ESXI_PASSWORD=… CYBERCTF_TEST_ESXI_DATASTORE=datastore1 CYBERCTF_TEST_ESXI_NETWORK='VM Network' \
//! cargo test real_server_labs -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use super::isothrum::{EsxiPlace, LabRun, ServerPlace, ServerRun};
use super::lab;
use super::server::HostProfile;

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is required"))
}

/// A fresh copy of the lab in `CYBERCTF_TEST_<what>_LAB`, as `name` in the temp folder, without
/// its generated files.
fn copy_lab(what: &str, name: &str) -> PathBuf {
    let src = PathBuf::from(var(&format!("CYBERCTF_TEST_{what}_LAB")));
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::process::Command::new("cp").arg("-R").arg(&src).arg(&dir).status().unwrap().success());
    let _ = std::fs::remove_dir_all(dir.join(".isoloom"));
    dir
}

#[tokio::test]
#[ignore = "needs a Proxmox server; see the module docs"]
async fn a_vm_lab_runs_on_proxmox_through_the_runner() {
    let dir = copy_lab("PVE", "cyberctf-pve-runner-lab");
    lab::prepare(&dir, lab::Target::Proxmox).unwrap();
    let endpoint = var("CYBERCTF_TEST_PVE_ENDPOINT");
    let host = endpoint.trim_start_matches("https://").split(':').next().unwrap().to_owned();
    let vars = [
        ("proxmox_endpoint", endpoint.clone()),
        ("proxmox_username", "root@pam".into()),
        ("proxmox_insecure", "true".into()),
        ("proxmox_ssh_address", host),
        ("proxmox_api_token", var("CYBERCTF_TEST_PVE_TOKEN")),
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
    let home = PathBuf::from(var("CYBERCTF_TEST_RUNNER_HOME"));
    let run = ServerRun::new(&home, &dir, "pve-runner-lab").unwrap();
    let mut log = |l: String| println!("[pve] {l}");
    let up = run.up(&place, &[], &mut log).await;
    let status = run.status(&place).await;
    let check = run.check().await;
    let down = run.down(&place, &mut log).await;
    up.expect("up");
    let status = status.expect("status");
    println!("status: running={} url={:?} machines={:?}", status.running, status.url, status.machines.iter().map(|m| (&m.name, &m.state)).collect::<Vec<_>>());
    assert!(status.running && status.url.is_some(), "running={} url={:?}", status.running, status.url);
    let check = check.expect("check");
    println!("checks: ok={} {:?}", check.ok, check.results.iter().map(|r| (&r.name, r.ok)).collect::<Vec<_>>());
    assert!(check.ok, "{}", check.output);
    down.expect("down");
    assert!(!run.status(&place).await.unwrap().running);
}

#[tokio::test]
#[ignore = "needs an ESXi host; see the module docs"]
async fn a_vm_lab_runs_on_esxi_through_the_runner() {
    let dir = copy_lab("ESXI", "cyberctf-esxi-runner-lab");
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
    let run = LabRun::on_esxi(&home, &dir, "esxi-runner-lab", EsxiPlace::from_env(&profile.id, &env)).unwrap();
    let mut log = |l: String| println!("[esxi] {l}");
    let up = run.up(&[], Some("vmware_esxi"), &mut log).await;
    let status = run.status().await;
    // Its entry, fetched while it runs.
    let fetched = status.as_ref().ok().and_then(|s| s.url.clone()).map(|url| {
        let out = std::process::Command::new("/usr/bin/curl").args(["-s", "-m", "15", "-o", "/dev/null", "-w", "%{http_code}", &url]).output().unwrap();
        (url, String::from_utf8_lossy(&out.stdout).into_owned())
    });
    let check = run.check().await;
    let provision = run.provision(None, &mut log).await;
    let down = run.down(&mut log).await;
    up.expect("up");
    let status = status.expect("status");
    println!("status: running={} url={:?} machines={:?}", status.running, status.url, status.machines.iter().map(|m| (&m.name, &m.state)).collect::<Vec<_>>());
    let (url, code) = fetched.expect("its entry");
    assert!(status.running && !url.contains("127.0.0.1"), "running={} url={url}", status.running);
    assert_eq!(code, "200", "{url} answers");
    let check = check.expect("check");
    println!("checks: ok={} {:?}", check.ok, check.results.iter().map(|r| (&r.name, r.ok)).collect::<Vec<_>>());
    assert!(check.ok, "{}", check.output);
    provision.expect("provision");
    down.expect("down");
    assert!(!run.status().await.unwrap().running);
}
