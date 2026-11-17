//! What a player does that breaks a VM lab, on the real Vagrant and hypervisor: a VM destroyed
//! (`vagrant destroy` in a terminal) or powered off (`vagrant halt --force`, as pulling its plug)
//! while the lab runs, the process running the start killed (SIGKILL) while it provisions, and
//! (on a throwaway machine only) a VM killed by its hypervisor's own tool while `vagrant up`
//! provisions it (`virsh destroy`, `VBoxManage controlvm … poweroff`, `prlctl stop --kill`,
//! `utmctl stop --force`, Hyper-V's `Stop-VM -TurnOff`: the rig's `CYBERCTF_TEST_RESILIENCE_VM_KILL`).
//! Each checks the lab page's status and the failure's words, that a resume or a start again
//! brings the lab back whole, and that its stop leaves no VM. The Docker family, and what both
//! share, is `resilience`.
//!
//! ```text
//! CYBERCTF_TEST_RESILIENCE_VM_LAB=<isoloom checkout>/examples/solo-web CYBERCTF_TEST_VM_PROVIDER=libvirt \
//! cargo test --lib integration_tests::resilience_vm:: -- --ignored --nocapture --test-threads=1
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::resilience::{ended_by_the_start_again, killed_start, refused_as_gone, shell, started_by, states, target, throwaway, whole_after};
use super::support::lab::{Run, every_machine_runs, vm_inputs};
use super::support::{Wait, check, eventually, knob, log, refused_by_launcher, scenario};
use crate::platform::exec::run_env;
use crate::runtime::providers::Provider;
use crate::runtime::{LabStatus, docker, isothrum, lab, launch, lifecycle};

/// The hypervisor under test (`CYBERCTF_TEST_VM_PROVIDER`, default QEMU, as the runner's).
fn provider() -> Provider {
    super::support::vm_provider(Provider::Qemu)
}

/// The run's Vagrant record (`VAGRANT_DOTFILE_PATH`), in the runner's home.
fn dotfile(l: &Run) -> PathBuf {
    l.home.join("runs").join(docker::project(&l.id)).join("vagrant")
}

/// The lab's status, as its page reads it.
async fn status(l: &Run) -> crate::error::Result<LabStatus> {
    isothrum::Deployment::vm(&l.home, &l.dir, &l.id)?.status().await
}

/// `machine`'s state as the lab page shows it.
async fn shown(l: &Run, machine: &str) -> Option<String> {
    status(l).await.ok()?.machines.into_iter().find(|m| m.name == machine).map(|m| m.state)
}

/// `vagrant <args>` on the run's VMs, as a player would in its folder.
async fn vagrant(l: &Run, args: &[&str]) -> crate::error::Result<String> {
    let env = [("VAGRANT_DOTFILE_PATH".to_owned(), dotfile(l).to_string_lossy().into_owned())];
    run_env("vagrant", args, Some(&lab::vagrant_dir(&l.dir)), &env).await
}

/// The lab started on the hypervisor (the lab page's Start), then up, its URL answering.
async fn starts_whole(l: &Run, f: &mut Vec<String>, when: &str) -> bool {
    let inputs = vm_inputs();
    let start = launch::VmStart { provider: provider(), inputs: launch::Inputs::values(&inputs) };
    let started = launch::start_vm_run(&l.home, &l.dir, &l.id, start, &mut log(&l.id)).await;
    whole_after(started, async || status(l).await, f, when).await
}

/// The lab stopped (the lab page's Stop), and no VM of it left.
async fn stops_clean(l: &Run, f: &mut Vec<String>) {
    let stopped = match isothrum::Deployment::vm(&l.home, &l.dir, &l.id) {
        Ok(run) => lifecycle::stop_run(run, &l.dir, log(&l.id)).await,
        Err(e) => Err(e),
    };
    check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), f);
    let s = status(l).await;
    check(s.as_ref().is_ok_and(|s| s.machines.is_empty()), format!("no VM of it is left ({:?})", s.as_ref().map(states)), f);
    l.remove_files();
}

/// Runs `body` on a fresh copy of the lab in `CYBERCTF_TEST_RESILIENCE_VM_LAB`, then stops it and
/// checks no VM is left, whatever happened; a lab the launcher refuses on the hypervisor is skipped.
async fn on_lab(name: &str, body: impl AsyncFnOnce(&Run, &mut Vec<String>)) {
    let Some(src) = knob("CYBERCTF_TEST_RESILIENCE_VM_LAB") else { return };
    let l = Run::fresh(Path::new(&src), "resvm");
    if refused_by_launcher(&l.dir, lab::Target::Vagrant, provider()) {
        l.remove_files();
        return;
    }
    let mut f = Vec::new();
    scenario(&mut f, async |f: &mut Vec<String>| body(&l, f).await, async |f: &mut Vec<String>| stops_clean(&l, f).await).await;
    assert!(f.is_empty(), "{name} ({}):\n{}", provider().id(), f.join("\n"));
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a VM lab (CYBERCTF_TEST_RESILIENCE_VM_LAB)"]
async fn a_vm_destroyed_while_the_lab_runs_shows_gone_and_a_start_again_rebuilds_it() {
    on_lab("VM destroyed while running", async |l: &Run, f: &mut Vec<String>| {
        if !starts_whole(l, f, "first").await {
            return;
        }
        let machine = target();
        let destroyed = vagrant(l, &["destroy", "-f", &machine]).await;
        check(destroyed.is_ok(), format!("vagrant destroy -f {machine} ({:?})", destroyed.err()), f);
        let state = shown(l, &machine).await;
        check(state.as_deref() == Some("absent"), format!("{machine} is shown gone ({state:?})"), f);
        // A resume never makes it again from its box, unprovisioned: refused, naming it.
        let resumed = isothrum::Deployment::vm(&l.home, &l.dir, &l.id).expect("its run").resume(&mut log(&l.id)).await;
        check(refused_as_gone(&resumed, &machine), format!("a resume is refused, naming {machine} ({:?})", resumed.err().map(|e| e.to_string())), f);
        let again = shown(l, &machine).await;
        check(again.as_deref() == Some("absent") || again.is_none(), format!("{machine} wasn't made again ({again:?})"), f);
        // A start again clears what is left and builds the lab afresh.
        starts_whole(l, f, "started again").await;
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a VM lab (CYBERCTF_TEST_RESILIENCE_VM_LAB)"]
async fn a_vm_powered_off_while_the_lab_runs_shows_off_and_a_resume_brings_it_back() {
    on_lab("VM powered off while running", async |l: &Run, f: &mut Vec<String>| {
        if !starts_whole(l, f, "first").await {
            return;
        }
        let machine = target();
        let off = vagrant(l, &["halt", "--force", &machine]).await;
        check(off.is_ok(), format!("vagrant halt --force {machine} ({:?})", off.err()), f);
        let state = shown(l, &machine).await;
        check(state.as_deref().is_some_and(|s| s != "running" && s != "absent"), format!("{machine} is shown off ({state:?})"), f);
        let resumed = isothrum::Deployment::vm(&l.home, &l.dir, &l.id).expect("its run").resume(&mut log(&l.id)).await;
        check(resumed.is_ok(), format!("a resume brings it back ({:?})", resumed.err()), f);
        let s = status(l).await;
        check(s.as_ref().is_ok_and(every_machine_runs), format!("every machine runs again ({:?})", s.as_ref().map(states)), f);
    })
    .await;
}

/// Whether Vagrant has made a VM of the run yet (its record names one on the hypervisor).
fn has_a_vm(l: &Run, provider: Provider) -> Option<(String, String)> {
    let machines = std::fs::read_dir(dotfile(l).join("machines")).ok()?;
    machines.flatten().find_map(|m| {
        let id = std::fs::read_to_string(m.path().join(provider.id()).join("id")).ok()?;
        Some((m.file_name().to_string_lossy().into_owned(), id.trim().to_owned()))
    })
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a VM lab (CYBERCTF_TEST_RESILIENCE_VM_LAB)"]
async fn a_vm_start_killed_while_it_provisions_is_failed_and_a_start_again_works() {
    on_lab("VM start killed", async |l: &Run, f: &mut Vec<String>| {
        let p = provider();
        let begun = async || has_a_vm(l, p).is_some();
        let left = killed_start(l, "vm", p.id(), begun, f).await;
        starts_whole(l, f, "started again after the kill").await;
        ended_by_the_start_again(&left, f).await;
    })
    .await;
}

#[tokio::test]
#[ignore = "kills a VM by its hypervisor's tool: a throwaway machine (CYBERCTF_TEST_RESILIENCE_THROWAWAY=1)"]
async fn a_vm_killed_by_its_hypervisor_while_it_provisions_fails_the_start_and_a_start_again_works() {
    if !throwaway() {
        return;
    }
    let Some(kill) = knob("CYBERCTF_TEST_RESILIENCE_VM_KILL") else { return };
    on_lab("VM killed while provisioning", async |l: &Run, f: &mut Vec<String>| {
        let p = provider();
        // As soon as Vagrant has made the first VM (it boots and provisions it next), its
        // hypervisor kills it.
        let kill_it = async {
            let made = eventually("Vagrant's first VM", Wait::secs(3600, Duration::from_secs(1)), async || has_a_vm(l, p).ok_or(())).await;
            let Ok((vm, id)) = made else { return false };
            // A time, not a condition, on purpose: the kill must land while Vagrant boots and
            // provisions the VM it just made, and nothing a scenario can see marks that moment.
            tokio::time::sleep(Duration::from_secs(20)).await;
            println!("note: killing {vm} ({id}) by the hypervisor");
            shell(&kill.replace("{id}", &id)).await.is_ok()
        };
        let inputs = vm_inputs();
        let start = launch::VmStart { provider: p, inputs: launch::Inputs::values(&inputs) };
        let mut said = log(&l.id);
        let (started, killed) = tokio::join!(launch::start_vm_run(&l.home, &l.dir, &l.id, start, &mut said), kill_it);
        check(killed, "a VM was killed by its hypervisor while the start ran".into(), f);
        let s = status(l).await;
        if let Err(e) = &started {
            println!("note: the start failed: {e}");
            check(!s.as_ref().is_ok_and(every_machine_runs), format!("the lab isn't read as whole ({:?})", s.as_ref().map(states)), f);
        } else {
            // Vagrant booted it again on its own and finished: the lab must then be whole.
            println!("note: the start went through all the same");
            check(s.as_ref().is_ok_and(every_machine_runs), format!("a start that went through is whole ({:?})", s.as_ref().map(states)), f);
        }
        // What this process started that still runs once the start returned (not the VMs: their
        // hypervisor's processes, which name the run too, are nobody's here).
        let left: Vec<String> = started_by(std::process::id()).into_iter().map(|p| p.line).collect();
        check(left.is_empty(), format!("no tool of the start is left running ({left:?})"), f);
        if started.is_err() {
            starts_whole(l, f, "started again").await;
        }
    })
    .await;
}
