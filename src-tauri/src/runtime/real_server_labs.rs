//! A real VM lab on a real Proxmox server through the launcher's runner path, end to end: the
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

use std::path::PathBuf;

use super::isothrum::{ServerPlace, ServerRun};
use super::lab;

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is required"))
}

#[tokio::test]
#[ignore = "needs a Proxmox server; see the module docs"]
async fn a_vm_lab_runs_on_proxmox_through_the_runner() {
    let src = PathBuf::from(var("CYBERCTF_TEST_PVE_LAB"));
    let dir = std::env::temp_dir().join("cyberctf-pve-runner-lab");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::process::Command::new("cp").arg("-R").arg(&src).arg(&dir).status().unwrap().success());
    let _ = std::fs::remove_dir_all(dir.join(".isoloom"));
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
