//! A container lab with VM recipes too, run on this machine as one VM per machine (the player's
//! third local place in "Where should it run?"), end to end through the launcher's own start:
//! `runtime::start` with the lab's catalogue runtime (`DOCKER`) and the per-machine override, as
//! the `lab_launch` command's job runs it. Then the operations the lab page sends with that same
//! catalogue runtime (status, stop) must follow the VM form: the status reads a local VM with one
//! machine per spec machine, and the stop destroys the VMs and clears the form. Opt-in (it needs
//! Vagrant, a hypervisor and the boxes):
//!
//! ```text
//! CYBERCTF_TEST_PER_MACHINE_LAB=<isoloom checkout>/examples/supplier-portal-api \
//! CYBERCTF_TEST_VM_PROVIDER=qemu \
//! CYBERCTF_TEST_VM_INPUTS=API_URL=http://localhost:3206,LAUNCH_TOKEN=test-token \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 a_dual_form_lab_runs_as_one_vm_per_machine
//! ```
//!
//! `CYBERCTF_TEST_VM_INPUTS` gives the launch's values (`NAME=value,…`).

use std::path::Path;

use super::paths::{PER_MACHINE_MARKER, local_runtime, per_machine_vm};
use super::providers::Provider;
use super::real_labs::{check, log};
use super::{Place, Runtime, lab, status, stop_lab, vm};
use crate::testing::test_app::TestApp;

fn inputs() -> Vec<(String, String)> {
    std::env::var("CYBERCTF_TEST_VM_INPUTS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect()
}

/// The machines the spec declares (the VMs the run must have, besides the runner's own).
fn spec_machines(dir: &Path) -> Vec<String> {
    lab::load(dir).map(|s| s.machines.keys().cloned().collect()).unwrap_or_default()
}

/// The lab's status as the page reads it (by its catalogue runtime): up, in local VMs, one
/// running machine per spec machine.
async fn up_as_vms(t: &TestApp, dir: &Path, id: &str, f: &mut Vec<String>) {
    match status::status(&t.dirs(), dir, id, Runtime::Docker).await {
        Ok(s) => {
            check(s.running, "it reports running".into(), f);
            check(matches!(s.place, Some(Place::LocalVm)), "it runs in local VMs".into(), f);
            let machines: Vec<&str> = s.machines.iter().filter(|m| !m.infra).map(|m| m.name.as_str()).collect();
            for name in spec_machines(dir) {
                let m = s.machines.iter().find(|m| m.name == name);
                check(m.is_some_and(|m| m.state == "running"), format!("machine {name} is a running VM (found {machines:?})"), f);
            }
            // The web machine's fixed host port, forwarded from its VM: the portal answers here.
            match s.url {
                Some(url) => {
                    let res = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap().get(&url).send().await;
                    check(res.as_ref().is_ok_and(|r| r.status().is_success()), format!("{url} answers ({:?})", res.map(|r| r.status())), f);
                }
                None => check(false, "it has a URL on this machine (the web VM's forwarded port)".into(), f),
            }
        }
        Err(e) => check(false, format!("its status reads: {e}"), f),
    }
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a dual-form lab (CYBERCTF_TEST_PER_MACHINE_LAB)"]
async fn a_dual_form_lab_runs_as_one_vm_per_machine_end_to_end() {
    let Ok(src) = std::env::var("CYBERCTF_TEST_PER_MACHINE_LAB") else { return };
    let provider: Provider =
        serde_json::from_value(serde_json::json!(std::env::var("CYBERCTF_TEST_VM_PROVIDER").unwrap_or_else(|_| "virtualbox".into()))).unwrap();
    let t = TestApp::new();
    let id = format!("pm-{}", rand::random::<u16>());
    // Installed as the launcher installs a lab: under the app data folder, nothing generated.
    let dir = t.data().join("labs").join(&id);
    lab::copy_tree(Path::new(&src), &dir).unwrap_or_else(|e| panic!("copy {src} to {}: {e}", dir.display()));
    let started = std::time::Instant::now();
    let mut f = Vec::new();
    // The launch as the job runs it: the catalogue's runtime, the hypervisor, the override.
    let up = super::start(&t.dirs(), &dir, &id, Runtime::Docker, Some(provider), None, true, None, &inputs(), log(&id)).await;
    match up {
        Ok(()) => {
            check(per_machine_vm(&dir) && dir.join(PER_MACHINE_MARKER).is_file(), "the form is recorded with the lab".into(), &mut f);
            check(local_runtime(&dir, Runtime::Docker) == Runtime::Vm, "the lab runs here as a VM lab".into(), &mut f);
            check(lab::vagrant_dir(&dir, Runtime::Vm).join("Vagrantfile").is_file(), "the vagrant target's files were generated".into(), &mut f);
            check(!lab::vagrant_dir(&dir, Runtime::Docker).exists(), "not docker-vm's".into(), &mut f);
            up_as_vms(&t, &dir, &id, &mut f).await;
        }
        Err(e) => check(false, format!("it starts: {e}"), &mut f),
    }
    println!("per-machine ({}): up after {:.0?}", provider.id(), started.elapsed());
    // The page's Stop, with the catalogue runtime: the VMs go, and so does the form.
    let stopped = stop_lab(&t.dirs(), &id, Runtime::Docker, log(&id)).await;
    check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), &mut f);
    check(!per_machine_vm(&dir), "the form is forgotten".into(), &mut f);
    check(!vm::has_machines(&lab::vagrant_dir(&dir, Runtime::Vm)), "Vagrant tracks no machine of it".into(), &mut f);
    let gone = status::status(&t.dirs(), &dir, &id, Runtime::Docker).await;
    check(gone.as_ref().is_ok_and(|s| !s.running), format!("its status reads as gone ({:?})", gone.map(|s| s.running)), &mut f);
    println!("per-machine ({}): {:.0?}", provider.id(), started.elapsed());
    assert!(f.is_empty(), "per-machine lab failed:\n{}", f.join("\n"));
}
