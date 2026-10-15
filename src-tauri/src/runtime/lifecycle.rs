//! A lab after it started: parking (pause, shut down) and resuming it, re-running provisioning,
//! and stopping it wherever it runs (the auto-stop reaper, in `reaper`, stops expired ones).

use std::path::Path;

use crate::AppHandle;

use super::inflight::{Action, DeployGuard, lock_lab};
use super::paths::{lab_dir, local_vm, mark_local_vm, mark_parked, parked, runner_chosen, runner_home, state_dir, vagrant_dirs};
use super::{Park, Runtime, attack_vm, cloud_services, docker, isothrum, lab, server, terraform, vm};
use crate::error::{Error, Result};

/// Parks a lab running on this machine: its machines are kept and resume as they were, so a lab
/// that took an hour to build comes back in seconds (paused) or a boot (shut down), not a rebuild.
async fn park(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, mode: Park, mut log: impl FnMut(String)) -> Result<()> {
    let _lock = lock_lab(app, id).await?;
    let _deploy = DeployGuard::new(id, Action::Park);
    refuse_cloud_park(runtime)?;
    if server::lab_connection(app, dir)?.is_some() {
        return Err(Error::Invalid("Pausing a lab on a server host isn't available yet. Stop it instead.".into()));
    }
    match runtime {
        Runtime::Docker if local_vm(dir).is_some() => {
            refuse_vm_shutdown(mode)?;
            vm::park(&lab::vagrant_dir(dir, runtime), mode, &[], &mut log).await?;
        }
        Runtime::Docker if runner_chosen(app) => isothrum::LabRun::docker(&runner_home(app)?, dir, id)?.park(mode, &mut log).await?,
        Runtime::Docker => docker::park(dir, id, &mut log).await?,
        Runtime::Vm => {
            if let Err(e) = attack_vm::park(dir, mode, &mut log).await {
                log(format!("The attack VM didn't park: {e}"));
            }
            if runner_chosen(app) {
                isothrum::LabRun::vm(&runner_home(app)?, dir, id)?.park(mode, &mut log).await?;
            } else {
                vm::park(&lab::vagrant_dir(dir, runtime), mode, &[], &mut log).await?;
            }
        }
        Runtime::Cloud => unreachable!("refused above"),
    }
    mark_parked(dir, Some(mode))?;
    log(parked_message(mode).into());
    Ok(())
}

/// Cloud services are created or destroyed, never kept switched off.
fn refuse_cloud_park(runtime: Runtime) -> Result<()> {
    if runtime == Runtime::Cloud {
        return Err(Error::Invalid("A cloud lab can't be paused: stop it to end what it costs, and start it again later.".into()));
    }
    Ok(())
}

/// Inside the VM the containers don't restart on their own after a power-off, so a container
/// lab in a VM is paused (its state saved), never shut down.
fn refuse_vm_shutdown(mode: Park) -> Result<()> {
    if mode == Park::Shutdown {
        return Err(Error::Invalid("A container lab running in a VM can be paused, not shut down.".into()));
    }
    Ok(())
}

/// The last line of a park.
fn parked_message(mode: Park) -> &'static str {
    match mode {
        Park::Pause => "Lab paused. Resume it to pick up where you left off.",
        Park::Shutdown => "Lab shut down. Resume it to boot its machines again.",
    }
}

/// Brings a parked lab back, as it was.
async fn resume(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, mut log: impl FnMut(String)) -> Result<()> {
    let _lock = lock_lab(app, id).await?;
    let _deploy = DeployGuard::new(id, Action::Start);
    resume_locked(app, dir, id, runtime, &mut log).await
}

/// The body of `resume`, for a caller already holding the lab lock (a start on a parked lab).
/// Also the repair for a lab some machines of which went down (powered off by hand, or a cut
/// resume): the machines already up are left alone, the others come back.
pub(super) async fn resume_locked(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, log: &mut impl FnMut(String)) -> Result<()> {
    refuse_cloud_park(runtime)?;
    if server::lab_connection(app, dir)?.is_some() {
        return Err(Error::Invalid("This lab was parked on a server host; resuming there isn't available yet.".into()));
    }
    log("Resuming the lab…".into());
    match runtime {
        Runtime::Docker if local_vm(dir).is_some() => vm::resume(&lab::vagrant_dir(dir, runtime), &[], &mut *log).await?,
        Runtime::Docker if runner_chosen(app) => isothrum::LabRun::docker(&runner_home(app)?, dir, id)?.resume(&mut *log).await?,
        Runtime::Vm if runner_chosen(app) => {
            isothrum::LabRun::vm(&runner_home(app)?, dir, id)?.resume(&mut *log).await?;
            resume_attack_vm(dir, &mut *log).await;
        }
        Runtime::Docker => {
            // Its stopped containers pruned (or Docker reset): nothing to bring back. Say so and
            // drop the parked mark, so the page offers a fresh start instead of a resume that
            // fails on "no container to start" every time.
            if docker::containers(dir, id).await.is_ok_and(|n| n == 0) {
                mark_parked(dir, None)?;
                return Err(Error::Invalid("Nothing is left of this lab to resume (its stopped containers are gone). Start it again for a fresh copy.".into()));
            }
            docker::resume(dir, id, &mut *log).await?;
        }
        Runtime::Vm => {
            let vagrant = lab::vagrant_dir(dir, runtime);
            if !vm::has_machines(&vagrant) {
                // As for a container lab whose containers are gone: the mark is dropped, so the
                // next start builds it fresh instead of failing the same way again.
                mark_parked(dir, None)?;
                return Err(Error::Invalid("Nothing is left of this lab to resume: its virtual machines are gone. Start it again to build it fresh.".into()));
            }
            vm::resume(&vagrant, &[], &mut *log).await?;
            resume_attack_vm(dir, &mut *log).await;
        }
        Runtime::Cloud => unreachable!("refused above"),
    }
    mark_parked(dir, None)?;
    log("Lab resumed.".into());
    Ok(())
}

/// The attack VM back beside its resumed lab; best effort, as the lab is back either way.
async fn resume_attack_vm(dir: &Path, log: &mut impl FnMut(String)) {
    if let Err(e) = attack_vm::resume(dir, &mut *log).await {
        log(format!("The lab is back, but its attack VM didn't resume: {e}. Start it from the lab page."));
    }
}

/// Runs a VM lab's provisioners again on its running machines (one, or all): the repair for a
/// machine whose provisioning was cut short, without rebuilding the lab. Inputs the lab declared
/// (`isoloom.yml` `inputs`) were given at launch and aren't kept, so a lab that provisions from
/// them gets none here; the labs so far declare none.
async fn provision(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, machine: Option<&str>, mut log: impl FnMut(String)) -> Result<()> {
    if runtime != Runtime::Vm {
        return Err(Error::Invalid("Container labs aren't provisioned in place: use Reset for a clean copy.".into()));
    }
    let _lock = lock_lab(app, id).await?;
    let _deploy = DeployGuard::new(id, Action::Start);
    let conn = server::lab_connection(app, dir)?;
    if conn.as_ref().is_some_and(|c| server::terraform_target(c.provider).is_some()) {
        return Err(Error::Invalid("Re-running provisioning isn't available for labs on Proxmox or a cloud yet.".into()));
    }
    if conn.is_none() && runner_chosen(app) {
        let run = isothrum::LabRun::vm(&runner_home(app)?, dir, id)?;
        log(provision_target(&run.status().await?, machine)?);
        run.provision(machine, &mut log).await?;
        log("Provisioning done.".into());
        return Ok(());
    }
    let env: Vec<(String, String)> = conn.map(|c| c.env).unwrap_or_default();
    let vagrant = lab::vagrant_dir(dir, runtime);
    let status = vm::status(&vagrant, &env).await?;
    log(provision_target(&status, machine)?);
    vm::provision(&vagrant, machine, &env, &mut log).await?;
    log("Provisioning done.".into());
    Ok(())
}

/// Whether provisioning can run on `machine` (or every machine) of a lab in `status`, and the
/// line that says so.
fn provision_target(status: &super::LabStatus, machine: Option<&str>) -> Result<String> {
    if !status.running {
        return Err(Error::Invalid("Start (or resume) the lab first: provisioning runs on its running machines.".into()));
    }
    if let Some(m) = machine
        && !status.machines.iter().any(|x| x.name == m)
    {
        return Err(Error::Invalid(format!("this lab has no machine `{m}`")));
    }
    Ok(match machine {
        Some(m) => format!("Re-running provisioning on {m}…"),
        None => "Re-running provisioning on every machine…".into(),
    })
}

/// Stops a lab wherever it runs, destroying remote VMs so the next start is clean.
async fn stop(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let _lock = lock_lab(app, id).await?;
    stop_locked(app, dir, id, runtime, log).await
}

/// The body of `stop`, assuming the caller already holds the lab lock. The reaper uses this so
/// it can re-check expiry under the same lock before tearing a lab down (the lock is not
/// reentrant, so it must not call `stop`, which would deadlock).
pub(super) async fn stop_locked(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let _deploy = DeployGuard::new(id, Action::Stop);
    // The website's co-location check for this lab has nothing left to vouch for, and its hosts'
    // keys go with its hosts.
    crate::account::colocation::retire(id);
    super::paths::forget_lab_host_keys(dir);
    let Some(c) = server::lab_connection(app, dir)? else {
        if runtime == Runtime::Docker && local_vm(dir).is_none() && runner_chosen(app) {
            return stop_docker_run(&runner_home(app)?, dir, id, log).await;
        }
        if runtime == Runtime::Vm && runner_chosen(app) {
            return stop_vm_run(&runner_home(app)?, dir, id, log).await;
        }
        return stop_here(dir, id, runtime, log).await;
    };
    let result = match server::terraform_target(c.provider) {
        // Cloud services: destroyed on the account they run on.
        _ if runtime == Runtime::Cloud => cloud_services::destroy(app, dir, id, &c, log).await,
        // A Proxmox VM lab the runner runs: destroyed with the params it was applied with.
        Some(_) if super::launch::runner_runs(app, c.provider, runtime) => {
            let mut log = log;
            stop_server_run(app, dir, id, &c, &mut log).await
        }
        Some(tf) => {
            let module = lab::terraform_to_destroy(dir, runtime, tf)?;
            terraform::destroy(&module, &state_dir(app, id, tf)?, &c.tf_vars, &c.tf_env, log).await
        }
        None => vm::stop(&lab::vagrant_dir(dir, runtime), &c.env, log).await,
    };
    if result.is_ok() {
        forget_run(dir)?;
    }
    result
}

/// Destroys a Proxmox VM lab the runner runs (connection `c`, the server's as it is now).
async fn stop_server_run(app: &AppHandle, dir: &Path, id: &str, c: &server::Connection, log: &mut impl FnMut(String)) -> Result<()> {
    let host = server::marked_host(dir).unwrap_or_default();
    let run = isothrum::ServerRun::new(&runner_home(app)?, dir, id)?;
    let place = super::launch::server_place(app, &host, c, &run).await?;
    run.down(&place, log).await
}

/// Stops a lab that runs on this machine (its containers, a local VM, or its VMs) and, once it
/// is gone, clears what marked it as running. Takes the lab folder (no app handle), so the
/// real-lab tests run exactly this.
pub(super) async fn stop_here(dir: &Path, id: &str, runtime: Runtime, mut log: impl FnMut(String)) -> Result<()> {
    let result = match runtime {
        Runtime::Docker if local_vm(dir).is_some() => vm::stop(&lab::vagrant_dir(dir, runtime), &[], log).await,
        Runtime::Docker => docker::stop(dir, id, log).await,
        Runtime::Vm => {
            // The learner's attack VM goes with the lab it was started beside. The lab goes
            // even when it couldn't; the lab then still reads as running, so Stop retries both.
            let attacker = attack_vm::stop(dir, &mut log).await;
            vm::stop(&lab::vagrant_dir(dir, runtime), &[], log).await.and(attacker)
        }
        // Cloud services run on a cloud account only: nothing of them here.
        Runtime::Cloud => Ok(()),
    };
    if result.is_ok() {
        forget_run(dir)?;
    }
    result
}

/// Stops a container lab the Isothrum runner runs (runner home `home`) and, once it is gone,
/// clears what marked it as running. Takes no app handle, so the real-lab tests run exactly this.
pub(super) async fn stop_docker_run(home: &std::path::Path, dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    isothrum::LabRun::docker(home, dir, id)?.down(&mut log).await?;
    forget_run(dir)
}

/// Stops a VM lab the Isothrum runner runs (runner home `home`), its attack VM with it, and once
/// both are gone clears what marked it as running. The lab goes even when its attack VM couldn't;
/// the lab then still reads as running, so Stop retries both. Takes no app handle, so the
/// real-lab tests run exactly this.
pub(super) async fn stop_vm_run(home: &std::path::Path, dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    let attacker = attack_vm::stop(dir, &mut log).await;
    isothrum::LabRun::vm(home, dir, id)?.down(&mut log).await.and(attacker)?;
    forget_run(dir)
}

/// A stopped lab's markers: where it ran, its local VM, parked.
fn forget_run(dir: &Path) -> Result<()> {
    server::mark_lab(dir, None)?;
    mark_local_vm(dir, None)?;
    mark_parked(dir, None)
}

/// Destroys a parked lab's machines before the lab folder is replaced by a newer version of the
/// lab (they'd otherwise be orphaned at the hypervisor, Vagrant having lost track of them).
pub async fn clear_parked(dir: &Path, id: &str, log: &impl Fn(String)) {
    if parked(dir).is_none() {
        return;
    }
    log("A parked copy of an older version of this lab is here: removing it…".into());
    let mut sink = |l: String| log(l);
    // A container lab here: its containers go by their Compose project, wherever (or whether)
    // its generated Compose file is (Isoloom labs keep it under .isoloom-<n>/, not at the root).
    if local_vm(dir).is_none() {
        let _ = docker::stop(dir, id, &mut sink).await;
    }
    for vdir in vagrant_dirs(dir) {
        let _ = vm::stop(&vdir, &[], &mut sink).await;
    }
    let _ = attack_vm::stop(dir, &mut sink).await;
    let _ = mark_parked(dir, None);
}

// The bodies of the lab operations the deploy worker runs (`deploy_worker::execute`), by lab id.

pub async fn park_lab(app: &AppHandle, id: &str, runtime: Runtime, mode: Park, log: impl FnMut(String)) -> Result<()> {
    park(app, &lab_dir(app, id)?, id, runtime, mode, log).await
}

pub async fn resume_lab(app: &AppHandle, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    resume(app, &lab_dir(app, id)?, id, runtime, log).await
}

pub async fn provision_lab(app: &AppHandle, id: &str, runtime: Runtime, machine: Option<&str>, log: impl FnMut(String)) -> Result<()> {
    provision(app, &lab_dir(app, id)?, id, runtime, machine, log).await
}

/// How lab `id` runs on this machine, for a Stop of what the Machine page found of it there
/// (`in_vm`: its VMs, else its containers). `None` when it isn't an installed lab that runs here
/// (removed since, the self-test, or marked as running on a server host): its leftovers then go
/// by name.
pub fn runtime_here(app: &AppHandle, id: &str, in_vm: bool) -> Option<Runtime> {
    let dir = lab_dir(app, id).ok()?;
    if server::marked_host(&dir).is_some_and(|h| !h.is_empty()) {
        return None;
    }
    Some(if in_vm && local_vm(&dir).is_none() { Runtime::Vm } else { Runtime::Docker })
}

/// Stops lab `id` wherever it runs: the one Stop of the lab page, the Machine page and the
/// hosted agent. Marked as stopping from the start: ending a deploy can take minutes (Terraform
/// is interrupted and waited for), during which the page and the quit guard must see a stop, and
/// no attack VM may start. A deploy still running in its worker (and an attack VM start beside
/// it) is ended first, so the teardown never races a `vagrant up` or `compose up` that would
/// recreate what it removes; then the lab is torn down under its lock.
pub async fn stop_lab(app: &AppHandle, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(app, id)?;
    let _stopping = DeployGuard::new(id, Action::Stop);
    crate::deploy_worker::end_workers(app, id).await;
    stop(app, &dir, id, runtime, log).await
}

#[cfg(test)]
mod tests {
    use super::{Park, clear_parked, park_lab, parked_message, provision_lab, provision_target, refuse_cloud_park, refuse_vm_shutdown, resume_lab, stop_lab};
    use crate::platform::exec::fake::{Tools, fail, ok};
    use crate::runtime::paths::{local_vm, mark_local_vm, mark_parked, parked};
    use crate::runtime::providers::Provider;

    use crate::runtime::test_support::status_with;
    use crate::runtime::{Runtime, server};
    use crate::testing::test_app::TestApp;

    const UP: &str = "1,dc01,state,running\n1,dc01,provider-name,virtualbox\n";

    #[tokio::test]
    async fn a_container_lab_parks_and_resumes_with_its_containers() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        let tools = Tools::rules(&[("ps --all", ok(r#"{"Service":"web","State":"exited"}"#))]);
        let mut log = Vec::new();
        park_lab(t.handle(), "web", Runtime::Docker, Park::Shutdown, |l| log.push(l)).await.unwrap();
        assert!(tools.called("docker stop -t 1 cyberctf-web-attacker") && tools.called("-p cyberctf-web stop"), "{:?}", tools.calls());
        assert_eq!(parked(&dir), Some(Park::Shutdown));
        assert_eq!(log.last().map(String::as_str), Some(parked_message(Park::Shutdown)));
        resume_lab(t.handle(), "web", Runtime::Docker, |l| log.push(l)).await.unwrap();
        assert!(tools.called("-p cyberctf-web start") && tools.called("docker start cyberctf-web-attacker"));
        assert_eq!(parked(&dir), None);
        assert_eq!(log.last().map(String::as_str), Some("Lab resumed."));
    }

    #[tokio::test]
    async fn a_container_lab_with_nothing_left_offers_a_fresh_start() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        mark_parked(&dir, Some(Park::Shutdown)).unwrap();
        let tools = Tools::rules(&[]);
        let err = resume_lab(t.handle(), "web", Runtime::Docker, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("Nothing is left of this lab to resume"), "{err}");
        assert_eq!(parked(&dir), None, "the parked mark is dropped");
        assert!(!tools.called(" start"));
        // A lab that isn't installed is refused before anything runs.
        assert!(resume_lab(t.handle(), "gone", Runtime::Docker, |_| {}).await.is_err());
        assert!(park_lab(t.handle(), "../x", Runtime::Docker, Park::Pause, |_| {}).await.is_err());
    }

    #[tokio::test]
    async fn a_container_lab_in_a_local_vm_pauses_and_resumes_the_vm() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        mark_local_vm(&dir, Some(Provider::Virtualbox)).unwrap();
        let tools = Tools::rules(&[]);
        let err = park_lab(t.handle(), "web", Runtime::Docker, Park::Shutdown, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("paused, not shut down"), "{err}");
        assert!(tools.calls().is_empty());
        park_lab(t.handle(), "web", Runtime::Docker, Park::Pause, |_| {}).await.unwrap();
        assert!(tools.called("vagrant suspend"));
        assert_eq!(parked(&dir), Some(Park::Pause));
        resume_lab(t.handle(), "web", Runtime::Docker, |_| {}).await.unwrap();
        assert!(tools.called("vagrant up --no-provision"));
        // Stopping it destroys the VM and forgets where it ran.
        stop_lab(t.handle(), "web", Runtime::Docker, |_| {}).await.unwrap();
        assert!(tools.called("vagrant destroy --force"));
        assert_eq!(local_vm(&dir), None);
    }

    #[tokio::test]
    async fn a_vm_lab_parks_resumes_and_stops_with_its_vms() {
        let t = TestApp::new();
        let dir = t.lab("ad", &[]);
        let tools = Tools::rules(&[("vagrant status", ok(UP))]);
        park_lab(t.handle(), "ad", Runtime::Vm, Park::Shutdown, |_| {}).await.unwrap();
        assert!(tools.called("vagrant halt"));
        // Its VMs deleted at the hypervisor (Vagrant's record of them gone): resuming would build
        // blank, unprovisioned machines. Refused, and the mark dropped so the next start is fresh.
        let err = resume_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("Nothing is left of this lab to resume"), "{err}");
        assert!(!tools.called("vagrant up"), "{:?}", tools.calls());
        assert_eq!(parked(&dir), None);
        // With its machines on record, it comes back as it was.
        mark_parked(&dir, Some(Park::Shutdown)).unwrap();
        record_vm(&dir, "dc01");
        resume_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.unwrap();
        assert!(tools.called("vagrant up --no-provision --provider virtualbox"), "{:?}", tools.calls());
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        stop_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.unwrap();
        assert!(tools.called("vagrant destroy --force"));
        assert_eq!(parked(&dir), None);
    }

    /// Vagrant's record of a machine of the lab (`.vagrant/machines/<name>/<provider>/id`).
    fn record_vm(dir: &std::path::Path, name: &str) {
        let record = crate::runtime::lab::vagrant_dir(dir, Runtime::Vm).join(".vagrant").join("machines").join(name).join("virtualbox");
        std::fs::create_dir_all(&record).unwrap();
        std::fs::write(record.join("id"), "0000").unwrap();
    }

    #[tokio::test]
    async fn an_attack_vm_whose_destroy_failed_stays_on_record_and_the_stop_says_so() {
        // Regression: the attack VM's `.vagrant` was deleted whatever `vagrant destroy` said; on
        // VMware, Hyper-V or libvirt the VM then ran on with nothing left to destroy it with.
        let t = TestApp::new();
        let dir = t.lab("ad", &[]);
        let attacker = crate::runtime::attack_vm::dir(&dir);
        std::fs::create_dir_all(attacker.join(".vagrant").join("machines").join("attacker").join("vmware_desktop")).unwrap();
        std::fs::write(attacker.join("Vagrantfile"), "").unwrap();
        let mut destroys = 0;
        let tools = Tools::new(move |_, args| match args {
            // The attack VM goes first: its destroy fails, the lab's own one works.
            ["destroy", ..] => {
                destroys += 1;
                if destroys == 1 { fail("The VMware VM is busy") } else { ok("") }
            }
            _ => ok(""),
        });
        let err = stop_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("The attack VM wasn't removed") && err.contains("busy"), "{err}");
        assert_eq!(tools.calls().iter().filter(|c| c.contains("destroy")).count(), 2, "the lab's VMs went anyway");
        assert!(attacker.join(".vagrant").exists(), "kept, for the next stop to retry");
        drop(tools);
        let _tools = Tools::rules(&[]);
        stop_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.unwrap();
        assert!(!attacker.join(".vagrant").exists());
    }

    #[tokio::test]
    async fn a_failed_park_or_stop_keeps_the_labs_marks() {
        let t = TestApp::new();
        let dir = t.lab("ad", &[]);
        record_vm(&dir, "dc01");
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        let _tools = Tools::rules(&[("vagrant up", fail("VBoxManage: error")), ("vagrant halt", fail("locked")), ("destroy", fail("locked"))]);
        assert!(resume_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.is_err());
        assert_eq!(parked(&dir), Some(Park::Pause));
        assert!(park_lab(t.handle(), "ad", Runtime::Vm, Park::Shutdown, |_| {}).await.is_err());
        assert_eq!(parked(&dir), Some(Park::Pause));
        assert!(stop_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.is_err());
        assert_eq!(parked(&dir), Some(Park::Pause));
    }

    #[tokio::test]
    async fn provisioning_reruns_on_a_running_vm_lab_only() {
        let t = TestApp::new();
        t.lab("ad", &[]);
        let tools = Tools::rules(&[("vagrant status", ok(UP))]);
        let err = provision_lab(t.handle(), "ad", Runtime::Docker, None, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("Container labs aren't provisioned in place"), "{err}");
        let mut log = Vec::new();
        provision_lab(t.handle(), "ad", Runtime::Vm, Some("dc01"), |l| log.push(l)).await.unwrap();
        assert!(tools.called("vagrant provision dc01"));
        assert_eq!(log.first().map(String::as_str), Some("Re-running provisioning on dc01…"));
        assert_eq!(log.last().map(String::as_str), Some("Provisioning done."));
        assert!(provision_lab(t.handle(), "ad", Runtime::Vm, Some("nope"), |_| {}).await.is_err());
    }

    #[tokio::test]
    async fn a_lab_on_a_cloud_account_is_neither_parked_nor_provisioned_there() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        let host = t.host("az1", "azure", serde_json::json!({}));
        server::mark_lab(&dir, Some(&host)).unwrap();
        let tools = Tools::rules(&[]);
        let err = park_lab(t.handle(), "web", Runtime::Docker, Park::Pause, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("Pausing a lab on a server host"), "{err}");
        let err = resume_lab(t.handle(), "web", Runtime::Docker, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("parked on a server host"), "{err}");
        let err = provision_lab(t.handle(), "web", Runtime::Vm, None, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("Proxmox or a cloud"), "{err}");
        assert!(tools.calls().is_empty());
        // Stopping it destroys it with Terraform there: with no state, nothing is left to destroy,
        // and the lab no longer reads as running there.
        stop_lab(t.handle(), "web", Runtime::Docker, |_| {}).await.unwrap();
        assert!(!dir.join(server::HOST_MARKER).exists());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_parked_lab_is_cleared_before_its_folder_is_replaced() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        mark_parked(&dir, Some(Park::Shutdown)).unwrap();
        let tools = Tools::rules(&[]);
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        clear_parked(&dir, "web", &|l: String| lines.lock().unwrap().push(l)).await;
        assert!(tools.called("down --volumes --remove-orphans"));
        assert_eq!(parked(&dir), None);
        assert!(lines.lock().unwrap()[0].starts_with("A parked copy of an older version"));
    }

    #[test]
    fn provisioning_needs_a_running_lab_and_a_known_machine() {
        let err = provision_target(&status_with(false, &["dc01"]), None).unwrap_err().to_string();
        assert!(err.starts_with("Start (or resume) the lab first"), "{err}");
        let up = status_with(true, &["dc01", "ws01"]);
        assert_eq!(provision_target(&up, Some("nope")).unwrap_err().to_string(), "this lab has no machine `nope`");
        assert_eq!(provision_target(&up, Some("ws01")).unwrap(), "Re-running provisioning on ws01…");
        assert_eq!(provision_target(&up, None).unwrap(), "Re-running provisioning on every machine…");
    }

    #[test]
    fn a_container_lab_in_a_vm_pauses_but_never_shuts_down() {
        assert!(refuse_vm_shutdown(Park::Pause).is_ok());
        assert!(refuse_vm_shutdown(Park::Shutdown).unwrap_err().to_string().contains("paused, not shut down"));
        assert!(parked_message(Park::Pause).starts_with("Lab paused."));
        assert!(parked_message(Park::Shutdown).starts_with("Lab shut down."));
    }

    #[test]
    fn a_cloud_lab_is_never_parked() {
        assert!(refuse_cloud_park(Runtime::Cloud).unwrap_err().to_string().starts_with("A cloud lab can't be paused"));
        assert!(refuse_cloud_park(Runtime::Docker).is_ok());
        assert!(refuse_cloud_park(Runtime::Vm).is_ok());
    }

    #[tokio::test]
    async fn a_stop_waits_for_the_operation_that_holds_the_lab() {
        // A stop asked for while a start is still in flight (in this process) must not tear
        // down what the start is creating: it waits for the lab, then runs.
        let t = TestApp::new();
        let id = "lifecycle-test-stop-waits";
        t.lab(id, &[]);
        let tools = Tools::rules(&[]);
        let held = crate::runtime::inflight::lock_lab(t.handle(), id).await.unwrap();
        let app = t.handle().clone();
        let mut stop = tokio::spawn(async move { stop_lab(&app, id, Runtime::Docker, |_| {}).await });
        assert!(tokio::time::timeout(std::time::Duration::from_millis(200), &mut stop).await.is_err(), "the stop waits for the lab");
        assert!(!tools.called("down --volumes"), "{:?}", tools.calls());
        drop(held);
        stop.await.unwrap().unwrap();
        assert!(tools.called("down --volumes"));
    }

    #[tokio::test]
    async fn a_park_whose_attack_vm_refuses_still_parks_the_lab_and_says_so() {
        let t = TestApp::new();
        let dir = t.lab("ad", &[]);
        let attack = crate::runtime::attack_vm::dir(&dir);
        std::fs::create_dir_all(&attack).unwrap();
        std::fs::write(attack.join("Vagrantfile"), "").unwrap();
        // The attacker is parked first: its `vagrant suspend` fails (an aborted VM can't be
        // suspended), the lab's own goes through.
        let mut suspends = 0;
        let tools = Tools::new(move |program, args| {
            if program == "vagrant" && args.first() == Some(&"suspend") {
                suspends += 1;
                return if suspends == 1 { fail("The VM is in the 'aborted' state") } else { ok("") };
            }
            ok("")
        });
        let mut log = Vec::new();
        park_lab(t.handle(), "ad", Runtime::Vm, Park::Pause, |l| log.push(l)).await.unwrap();
        assert_eq!(tools.calls().iter().filter(|c| c.starts_with("vagrant suspend")).count(), 2, "{:?}", tools.calls());
        assert!(log.iter().any(|l| l.starts_with("The attack VM didn't park")), "{log:?}");
        assert_eq!(parked(&dir), Some(Park::Pause));
        assert_eq!(log.last().map(String::as_str), Some(parked_message(Park::Pause)));
    }

    #[tokio::test]
    async fn a_park_cut_short_leaves_the_lab_unparked_and_a_resume_repairs_it() {
        // One machine refused to halt: the lab is half down. It isn't marked parked (a start
        // would wrongly resume it), and Resume brings the machines that went down back.
        let t = TestApp::new();
        let dir = t.lab("ad", &[]);
        record_vm(&dir, "dc01");
        let tools = Tools::rules(&[("vagrant halt", fail("ws01: The guest machine entered an invalid state")), ("vagrant status", ok(UP))]);
        assert!(park_lab(t.handle(), "ad", Runtime::Vm, Park::Shutdown, |_| {}).await.is_err());
        assert_eq!(parked(&dir), None, "half parked is not parked");
        resume_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.unwrap();
        assert!(tools.called("vagrant up --no-provision --provider virtualbox"), "{:?}", tools.calls());
        assert_eq!(parked(&dir), None);
    }

    #[tokio::test]
    async fn a_stop_whose_destroy_fails_keeps_every_marker_for_the_retry() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        // A container lab in a local VM: `vagrant destroy` fails half-way.
        mark_local_vm(&dir, Some(Provider::Virtualbox)).unwrap();
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        let tools = Tools::rules(&[("vagrant destroy", fail("VBoxManage: error: The machine is locked"))]);
        assert!(stop_lab(t.handle(), "web", Runtime::Docker, |_| {}).await.is_err());
        assert!(tools.called("vagrant destroy --force"));
        assert_eq!(local_vm(&dir).as_deref(), Some("virtualbox"), "stop, status and the shell still go to the VM");
        assert_eq!(parked(&dir), Some(Park::Pause));
        drop(tools);
        // On a cloud account: its state holds resources, but the module to destroy them with
        // is missing. The host marker stays, so the lab is still known to be up there.
        let host = t.host("az1", "azure", serde_json::json!({}));
        server::mark_lab(&dir, Some(&host)).unwrap();
        let state = t.data().join("deployments").join("web").join("azure");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}},"resources":[{}]}"#).unwrap();
        let tools = Tools::rules(&[]);
        let err = stop_lab(t.handle(), "web", Runtime::Docker, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("no Terraform module"), "{err}");
        assert!(!tools.called("terraform destroy"));
        assert_eq!(server::marked_host(&dir).as_deref(), Some("az1"));
        assert_eq!(crate::runtime::deployed_on_server(t.handle(), &dir, "web").as_deref(), Some("Host az1"));
        assert!(state.join("terraform.tfstate").is_file(), "the state is kept for the retry");
    }

    #[tokio::test]
    async fn provisioning_reaches_a_dotted_machine_and_refuses_one_the_lab_lacks_before_vagrant() {
        let t = TestApp::new();
        t.lab("ad", &[]);
        // Isoloom names Windows machines by their FQDN; the dot must reach `vagrant provision`.
        let dotted = "1,kingslanding.sevenkingdoms,state,running\n1,kingslanding.sevenkingdoms,provider-name,virtualbox\n";
        let tools = Tools::rules(&[("vagrant status", ok(dotted))]);
        provision_lab(t.handle(), "ad", Runtime::Vm, Some("kingslanding.sevenkingdoms"), |_| {}).await.unwrap();
        assert!(tools.called("vagrant provision kingslanding.sevenkingdoms"), "{:?}", tools.calls());
        // A name that would read as a Vagrant option passes the command's character check (dash
        // is allowed), but the lab has no such machine: refused before `vagrant provision` runs.
        let err = provision_lab(t.handle(), "ad", Runtime::Vm, Some("--provision-with"), |_| {}).await.unwrap_err().to_string();
        assert_eq!(err, "this lab has no machine `--provision-with`");
        assert_eq!(tools.calls().iter().filter(|c| c.starts_with("vagrant provision")).count(), 1);
    }

    #[tokio::test]
    async fn stopping_a_cloud_lab_that_runs_nowhere_clears_its_marks_without_running_anything() {
        // A cloud-services lab has nothing on this machine: a stop with no host marker (the
        // account was never recorded, or the marker was lost) only drops stale marks.
        let t = TestApp::new();
        let dir = t.lab("goat", &[]);
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        let tools = Tools::rules(&[]);
        stop_lab(t.handle(), "goat", Runtime::Cloud, |_| {}).await.unwrap();
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert_eq!(parked(&dir), None);
        // Parking or resuming it is refused on the runtime alone.
        let err = park_lab(t.handle(), "goat", Runtime::Cloud, Park::Pause, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
        let err = resume_lab(t.handle(), "goat", Runtime::Cloud, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
        assert!(tools.calls().is_empty());
    }

    #[tokio::test]
    async fn clearing_a_lab_that_is_not_parked_does_nothing() {
        let dir = std::env::temp_dir().join(format!("cyberctf-clear-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        clear_parked(&dir, "web-1", &|l: String| lines.lock().unwrap().push(l)).await;
        assert!(lines.lock().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
