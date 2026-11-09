//! A container lab in a local VM (Isoloom's docker-vm) through the Isothrum runner, end to end,
//! for the v1 release QA: start it on a local hypervisor through the launcher's own start
//! (`launch::start_docker_in_vm_run`, the path the lab page takes for a container lab in a VM),
//! read its status (running, in a local VM, its machine up), then stop the lab (the runner's
//! `down`) and assert no VM of it is left.
//! The attack box inside the VM is `real_attack_machine`'s. Opt-in (it needs Vagrant, a
//! hypervisor and its boxes):
//!
//! ```text
//! CYBERCTF_TEST_DOCKER_VM_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_VM_PROVIDER=virtualbox \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 a_container_lab_runs_in_a_local_vm
//! ```

use std::path::Path;

use super::support::{check, install_copy, leftovers, log};
use crate::runtime::providers::Provider;
use crate::runtime::{Place, isothrum, lab, launch, lifecycle};

/// The lab's status while it runs in the VM: running, placed in a local VM, its VM up. A docker-vm
/// lab's containers are up and healthy inside the VM (the start blocked on Compose's `--wait`
/// before returning) and are reached from a machine inside the VM, since the lab network
/// isn't reachable from this machine; so there is no host URL to fetch here, by design.
async fn up_in_vm(home: &Path, dir: &Path, id: &str, f: &mut Vec<String>) {
    match isothrum::LabRun::docker_in_vm(home, dir, id).unwrap().status().await {
        Ok(s) => {
            check(s.running, "it reports running".into(), f);
            check(matches!(s.place, Some(Place::LocalVm)), "it runs in a local VM".into(), f);
            for m in s.machines.iter().filter(|m| !m.infra) {
                check(m.state == "running", format!("machine {} is running (state {})", m.name, m.state), f);
            }
            check(!s.machines.is_empty(), "the VM is reported".into(), f);
        }
        Err(e) => check(false, format!("its status reads: {e}"), f),
    }
}

/// Whether the hypervisor still holds a VM of the lab in `dir` (by the name Isoloom gives it).
async fn left(provider: Provider, dir: &Path) -> Vec<String> {
    let name = lab::lab(dir).map(|l| l.source().name.clone()).unwrap_or_default();
    let listed = match provider {
        Provider::Virtualbox => crate::platform::exec::run("VBoxManage", &["list", "vms"], None).await,
        Provider::Utm => crate::platform::exec::run("/Applications/UTM.app/Contents/MacOS/utmctl", &["list"], None).await,
        _ => return Vec::new(),
    };
    listed.unwrap_or_default().lines().filter(|l| l.contains(&format!("{name} · ")) || l.starts_with(&format!("\"{name}_"))).map(str::to_owned).collect()
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a docker-vm-capable lab (CYBERCTF_TEST_DOCKER_VM_LAB)"]
async fn a_container_lab_runs_in_a_local_vm_end_to_end() {
    let Ok(src) = std::env::var("CYBERCTF_TEST_DOCKER_VM_LAB") else { return };
    let provider: Provider =
        serde_json::from_value(serde_json::json!(std::env::var("CYBERCTF_TEST_VM_PROVIDER").unwrap_or_else(|_| "virtualbox".into()))).unwrap();
    let id = format!("dvm-{}", rand::random::<u16>());
    let dir = install_copy(Path::new(&src), &id);
    let home = std::env::temp_dir().join(format!("cyberctf-{id}-home"));
    let started = std::time::Instant::now();
    let mut f = Vec::new();
    // The launcher's own start of a container lab in a VM, as the lab page's runs it: its Vagrant
    // files generated, the lab marked as running in a local VM, UTM opened first and its passing
    // refusal tried again, then up through the runner.
    let brought_up = launch::start_docker_in_vm_run(&home, &dir, &id, launch::VmStart { provider, env: &[] }, &mut log(&id)).await;
    match brought_up {
        Ok(()) => {
            up_in_vm(&home, &dir, &id, &mut f).await;
        }
        Err(e) => check(false, format!("it starts: {e}"), &mut f),
    }
    let stopped = lifecycle::stop_run(isothrum::LabRun::docker_in_vm(&home, &dir, &id).unwrap(), &dir, log(&id)).await;
    check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), &mut f);
    let leftover_vms = left(provider, &dir).await;
    check(leftover_vms.is_empty(), format!("no VM of it is left ({leftover_vms:?})"), &mut f);
    // No container of it on the host either (it ran inside the VM, not here).
    check(leftovers(&id).await.is_empty(), "nothing of it is left on this machine's Docker".into(), &mut f);
    let status_gone = isothrum::LabRun::docker_in_vm(&home, &dir, &id).unwrap().status().await;
    check(status_gone.is_ok_and(|s| !s.running), "its status reads as gone".into(), &mut f);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&home);
    println!("docker-vm ({}): {:.0?}", provider.id(), started.elapsed());
    assert!(f.is_empty(), "docker-vm lab failed:\n{}", f.join("\n"));
}
