//! A container lab in a local VM (Isoloom's docker-vm) through the Isothrum runner, end to end,
//! for the v1 release QA: start it on a local hypervisor (the runner's `docker_in_vm` path the
//! lab page takes when a container lab is run inside a VM), read its status (running, in a local
//! VM, its machine up), start the attack box beside the containers inside the VM and reach the
//! web target from a shell in it, take the box out, then stop the lab (the runner's `down`) and
//! assert no VM of it is left. Opt-in (it needs Vagrant, a hypervisor and its boxes):
//!
//! ```text
//! CYBERCTF_TEST_DOCKER_VM_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_VM_PROVIDER=virtualbox \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 a_container_lab_runs_in_a_local_vm
//! ```
//!
//! `CYBERCTF_TEST_ATTACK_BOX` overrides the attack-box image (default: the runner's own, Kali).

use std::path::Path;

use isothrum_runner::ShellCommand;

use super::providers::Provider;
use super::real_labs::{check, detach_box, install_copy, leftovers, log, vagrant_ssh};
use super::{Place, Runtime, isothrum, lab, lifecycle, paths};

/// The lab's status while it runs in the VM: running, placed in a local VM, its VM up. A docker-vm
/// lab's containers are up and healthy inside the VM (the start blocked on Compose's `--wait`
/// before returning) and are reached through the attack box inside the VM, since the lab network
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

/// The attack box inside the VM, as the lab page drives it: down with the lab network to join
/// known, attached beside the containers, reported running with an address on the lab network,
/// a shell through the VM into it that reaches the web target, then taken out.
async fn attack_box_in_vm(home: &Path, dir: &Path, id: &str, f: &mut Vec<String>) {
    let run = isothrum::LabRun::docker_in_vm(home, dir, id).unwrap();
    let image = std::env::var("CYBERCTF_TEST_ATTACK_BOX").ok();
    match run.attack_status(None).await {
        Ok(s) => check(!s.running && s.networks == ["app"], format!("the box starts down, the lab network to join known ({:?})", s.networks), f),
        Err(e) => check(false, format!("the box's status reads: {e}"), f),
    }
    if let Err(e) = run.attach(None, image.as_deref(), &mut log(id)).await {
        check(false, format!("the box attaches inside the VM: {e}"), f);
        return;
    }
    match run.attack_status(None).await {
        Ok(s) => {
            check(s.running, "the box reports running".into(), f);
            check(s.ip.starts_with("10.80.0."), format!("it has an address on the lab network ({})", s.ip), f);
            check(s.lab_network == "app", format!("on the lab network ({})", s.lab_network), f);
        }
        Err(e) => check(false, format!("the box's status reads: {e}"), f),
    }
    match run.shell_command() {
        Ok(shell) => shell_reaches_target(&shell, f).await,
        Err(e) => check(false, format!("the shell into the box is built: {e}"), f),
    }
    detach_box(&run, id, f).await;
}

/// The shell's command run non-interactively through the same way into the VM: a request to
/// `http://web/` from inside the box, over bash's `/dev/tcp` (nothing but a shell is assumed of
/// the image).
async fn shell_reaches_target(shell: &ShellCommand, f: &mut Vec<String>) {
    let through_vm = shell.program == "vagrant" && shell.args.len() == 4 && shell.args[0] == "ssh" && shell.args[2] == "-c";
    check(through_vm, format!("the shell goes through the VM into the box ({} {:?})", shell.program, shell.args), f);
    let Some(name) = shell.args.get(3).and_then(|exec| exec.split('\'').nth(1)) else { return };
    let vm = shell.args.get(1).map_or("default", String::as_str);
    let probe =
        format!("sudo docker exec {name} bash -c \"exec 3<>/dev/tcp/web/80 && printf 'GET / HTTP/1.0\\r\\nHost: web\\r\\n\\r\\n' >&3 && head -c 300 <&3\"");
    let out = vagrant_ssh(shell, vm, &probe).await;
    check(out.as_ref().is_ok_and(|o| o.contains("HTTP/1.") && o.contains(" 200 ")), format!("a shell in the box reaches http://web/ ({out:?})"), f);
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
    // The lab page's path for a container lab in a VM: generate its Vagrant files, mark it as
    // running in a local VM, and bring it up through the runner.
    let brought_up = async {
        lab::prepare(&dir, lab::vagrant_target(Runtime::Docker))?;
        paths::mark_local_vm(&dir, Some(provider))?;
        isothrum::LabRun::docker_in_vm(&home, &dir, &id)?.up(&[], Some(provider.id()), &mut log(&id)).await
    }
    .await;
    match brought_up {
        Ok(()) => {
            up_in_vm(&home, &dir, &id, &mut f).await;
            attack_box_in_vm(&home, &dir, &id, &mut f).await;
        }
        Err(e) => check(false, format!("it starts: {e}"), &mut f),
    }
    let stopped = lifecycle::stop_docker_in_vm_run(&home, &dir, &id, log(&id)).await;
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
