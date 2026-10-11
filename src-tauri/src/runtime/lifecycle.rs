//! A lab after it started: parking (pause, shut down) and resuming it, re-running provisioning,
//! stopping it wherever it runs, and the reaper that destroys expired cloud labs.

use std::path::Path;

use crate::AppHandle;
use tauri::Manager;

use super::inflight::{Action, DeployGuard, lock_lab, try_lock_lab};
use super::paths::{
    deployed_ids, deployment_states, lab_dir, local_vm, mark_local_vm, mark_parked, parked, recorded_host, recorded_runtime, state_dir, vagrant_dirs,
};
use super::{Park, Runtime, attack_vm, docker, lab, registry, server, terraform, vm};
use crate::error::{Error, Result};

/// Parks a lab running on this machine: its machines are kept and resume as they were, so a lab
/// that took an hour to build comes back in seconds (paused) or a boot (shut down), not a rebuild.
async fn park(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, mode: Park, mut log: impl FnMut(String)) -> Result<()> {
    let _lock = lock_lab(app, id).await;
    let _deploy = DeployGuard::new(id, Action::Park);
    if server::lab_connection(app, dir)?.is_some() {
        return Err(Error::Invalid("Pausing a lab on a server host isn't available yet. Stop it instead.".into()));
    }
    match runtime {
        Runtime::Docker if local_vm(dir).is_some() => {
            refuse_vm_shutdown(mode)?;
            vm::park(&lab::vagrant_dir(dir, runtime), mode, &[], &mut log).await?;
        }
        Runtime::Docker => docker::park(dir, id, &mut log).await?,
        Runtime::Vm => {
            if let Err(e) = attack_vm::park(dir, mode, &mut log).await {
                log(format!("The attack VM didn't park: {e}"));
            }
            vm::park(&lab::vagrant_dir(dir, runtime), mode, &[], &mut log).await?;
        }
    }
    mark_parked(dir, Some(mode))?;
    log(parked_message(mode).into());
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
    let _lock = lock_lab(app, id).await;
    let _deploy = DeployGuard::new(id, Action::Start);
    resume_locked(app, dir, id, runtime, &mut log).await
}

/// The body of `resume`, for a caller already holding the lab lock (a start on a parked lab).
/// Also the repair for a lab some machines of which went down (powered off by hand, or a cut
/// resume): the machines already up are left alone, the others come back.
pub(super) async fn resume_locked(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, log: &mut impl FnMut(String)) -> Result<()> {
    if server::lab_connection(app, dir)?.is_some() {
        return Err(Error::Invalid("This lab was parked on a server host; resuming there isn't available yet.".into()));
    }
    log("Resuming the lab…".into());
    match runtime {
        Runtime::Docker if local_vm(dir).is_some() => vm::resume(&lab::vagrant_dir(dir, runtime), &[], &mut *log).await?,
        Runtime::Docker => {
            // Its stopped containers pruned (or Docker reset): nothing to bring back. Say so and
            // drop the parked mark, so the page offers a fresh start instead of a resume that
            // fails on "no container to start" every time.
            if docker::containers(dir, id).await.is_ok_and(|n| n == 0) {
                mark_parked(dir, None)?;
                return Err(Error::Invalid("Nothing is left of this lab to resume (its stopped containers are gone). Start it again for a fresh copy.".into()));
            }
            docker::resume(dir, id, &mut *log).await?;
            // Recorded again, with its Compose project (labs started before it was recorded).
            if let Ok(spec) = lab::instanced(dir) {
                registry::record_docker(dir, &spec, docker::project(id));
            }
        }
        Runtime::Vm => {
            vm::resume(&lab::vagrant_dir(dir, runtime), &[], &mut *log).await?;
            if let Err(e) = attack_vm::resume(dir, &mut *log).await {
                log(format!("The lab is back, but its attack VM didn't resume: {e}. Start it from the lab page."));
            }
        }
    }
    mark_parked(dir, None)?;
    log("Lab resumed.".into());
    Ok(())
}

/// Runs a VM lab's provisioners again on its running machines (one, or all): the repair for a
/// machine whose provisioning was cut short, without rebuilding the lab. Inputs the lab declared
/// (`isoloom.yml` `inputs`) were given at launch and aren't kept, so a lab that provisions from
/// them gets none here; the labs so far declare none.
async fn provision(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, machine: Option<&str>, mut log: impl FnMut(String)) -> Result<()> {
    if runtime != Runtime::Vm {
        return Err(Error::Invalid("Container labs aren't provisioned in place: use Reset for a clean copy.".into()));
    }
    let _lock = lock_lab(app, id).await;
    let _deploy = DeployGuard::new(id, Action::Start);
    let conn = server::lab_connection(app, dir)?;
    if conn.as_ref().is_some_and(|c| server::terraform_target(c.provider).is_some()) {
        return Err(Error::Invalid("Re-running provisioning isn't available for labs on Proxmox or a cloud yet.".into()));
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
pub(super) async fn stop(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let _lock = lock_lab(app, id).await;
    stop_locked(app, dir, id, runtime, log).await
}

/// The body of `stop`, assuming the caller already holds the lab lock. The reaper uses this so
/// it can re-check expiry under the same lock before tearing a lab down (the lock is not
/// reentrant, so it must not call `stop`, which would deadlock).
async fn stop_locked(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let _deploy = DeployGuard::new(id, Action::Stop);
    let Some(c) = server::lab_connection(app, dir)? else { return stop_here(dir, id, runtime, log).await };
    let result = match server::terraform_target(c.provider) {
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

/// Stops a lab that runs on this machine (its containers, a local VM, or its VMs) and, once it
/// is gone, clears what marked it as running. Takes the lab folder (no app handle), so the
/// real-lab tests run exactly this.
pub(super) async fn stop_here(dir: &Path, id: &str, runtime: Runtime, mut log: impl FnMut(String)) -> Result<()> {
    let result = match runtime {
        Runtime::Docker if local_vm(dir).is_some() => vm::stop(&lab::vagrant_dir(dir, runtime), &[], log).await,
        Runtime::Docker => docker::stop(dir, id, log).await,
        Runtime::Vm => {
            // The learner's attack VM goes with the lab it was started beside.
            attack_vm::stop(dir, &mut log).await;
            vm::stop(&lab::vagrant_dir(dir, runtime), &[], log).await
        }
    };
    if result.is_ok() {
        forget_run(dir)?;
    }
    result
}

/// A stopped lab's markers: where it ran, its local VM, Isoloom's registry entry, parked.
fn forget_run(dir: &Path) -> Result<()> {
    server::mark_lab(dir, None)?;
    mark_local_vm(dir, None)?;
    registry::forget(dir);
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
    attack_vm::stop(dir, &mut sink).await;
    let _ = mark_parked(dir, None);
}

/// Destroys cloud labs whose auto-stop time has passed, to end billing: on Azure an OS
/// poweroff only stops (not deallocates) the VM, so it keeps billing until a `destroy`, and a
/// destroy also cleans up leftover resources on every cloud. Runs on startup and periodically
/// while the app is open; a lab that expired while the app was closed is reaped at the next sweep.
/// Every state of a lab counts, not only the one its marker points at: a state left on a host
/// the lab no longer runs on is destroyed with the host it was applied on.
pub async fn reap_expired_labs(app: &AppHandle) {
    let Ok(base) = app.path().app_data_dir() else { return };
    for (id, target, state) in expired_states(&base) {
        let Ok(dir) = lab_dir(app, &id) else { continue };
        let marked = server::marked_host(&dir).and_then(|h| Some((server::terraform_target(server::host_provider(app, &h)?)?, h)));
        let Some(owner) = reap_owner(marked.as_ref().map(|(tf, h)| (*tf, h.as_str())), recorded_host(&state).as_deref(), &target) else {
            log::warn!("expired Terraform state of {id} ({target}) names no host to destroy it with");
            continue;
        };
        // Take the lab lock before tearing it down, and re-check expiry while holding it: between
        // the cheap filter above and here a user may have restarted or extended this lab (a start
        // re-applies and writes a fresh expiry), and destroying their live lab would be wrong.
        // A lab busy with another operation (a deploy worker's, maybe for hours) is left to the
        // next sweep rather than holding up the others.
        let Some(_lock) = try_lock_lab(app, &id) else { continue };
        if !terraform::expired(&state) {
            continue;
        }
        // Best effort: tear it down to end billing, with the runtime recorded at start. No UI
        // context here, so logs are dropped; if the destroy fails, the next sweep retries.
        let runtime = recorded_runtime(&state);
        match owner {
            Owner::Current => {
                let _ = stop_locked(app, &dir, &id, runtime, |_line: String| {}).await;
            }
            // Left behind: destroyed on its own host; the lab's markers (where it runs now) stay.
            Owner::Left(host) => {
                let Ok(c) = server::connection(app, &host) else { continue };
                let Ok(module) = lab::terraform_to_destroy(&dir, runtime, &target) else { continue };
                if let Err(e) = terraform::destroy(&module, &state, &c.tf_vars, &c.tf_env, |_line: String| {}).await {
                    log::warn!("destroying the expired leftover of {id} on {host}: {e}");
                }
            }
        }
    }
}

/// Every expired Terraform state under the app data folder `base`: lab id, target, folder.
fn expired_states(base: &Path) -> Vec<(String, String, std::path::PathBuf)> {
    deployed_ids(base)
        .into_iter()
        .flat_map(|id| deployment_states(base, &id).into_iter().map(move |(target, state)| (id.clone(), target, state)))
        .filter(|(_, _, state)| terraform::expired(state))
        .collect()
}

/// Whose an expired state is.
#[derive(Debug, PartialEq)]
enum Owner {
    /// The lab's current deployment (its marker names a host of this target).
    Current,
    /// Left on this host, which the lab no longer runs on.
    Left(String),
}

/// Whose the expired state of `target` is: the current deployment's when the lab is marked
/// (`marked`: the marked host's target and id) as running on a host of that target, else the
/// leftover of the host recorded with the state (`recorded`). `None` when no host is known
/// (a state from before hosts were recorded).
fn reap_owner(marked: Option<(&str, &str)>, recorded: Option<&str>, target: &str) -> Option<Owner> {
    match (marked, recorded) {
        (Some((tf, host)), recorded) if tf == target && recorded.is_none_or(|r| r == host) => Some(Owner::Current),
        (_, Some(recorded)) => Some(Owner::Left(recorded.to_string())),
        _ => None,
    }
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

/// Stops a lab with no log sink, for a caller that only needs the teardown (the hosted agent
/// cleaning up after a launch that started infra but then failed to report back).
pub async fn stop_lab(app: &AppHandle, id: &str, runtime: Runtime) -> Result<()> {
    stop(app, &lab_dir(app, id)?, id, runtime, |_l: String| {}).await
}

#[cfg(test)]
mod tests {
    use super::{
        Owner, Park, clear_parked, expired_states, park_lab, parked_message, provision_lab, provision_target, reap_expired_labs, reap_owner,
        refuse_vm_shutdown, resume_lab, stop_lab,
    };
    use crate::exec::fake::{Tools, fail, ok};
    use crate::runtime::paths::{local_vm, mark_local_vm, mark_parked, parked};
    use crate::runtime::providers::Provider;

    #[test]
    fn the_reaper_finds_expired_states_left_on_another_host() {
        // Regression: the reaper only looked at the state of the host a lab's marker named, so a
        // cloud deployment left behind when the lab was started elsewhere was never destroyed.
        let base = std::env::temp_dir().join(format!("cyberctf-reap-{}", rand::random::<u32>()));
        let state = |id: &str, target: &str, expires_at: u64| {
            let s = base.join("deployments").join(id).join(target);
            std::fs::create_dir_all(&s).unwrap();
            std::fs::write(s.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}},"resources":[{}]}"#).unwrap();
            std::fs::write(s.join("run.json"), format!(r#"{{"expires_at":{expires_at}}}"#)).unwrap();
            s
        };
        let left = state("web-1", "aws", 1);
        state("web-1", "proxmox", u64::MAX);
        let other = state("db-1", "azure", 1);
        let mut found = expired_states(&base);
        found.sort();
        assert_eq!(found, [("db-1".to_string(), "azure".to_string(), other), ("web-1".to_string(), "aws".to_string(), left)]);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn an_expired_state_is_destroyed_with_the_host_it_was_applied_on() {
        // The lab still runs where its marker says: the usual stop.
        assert_eq!(reap_owner(Some(("aws", "aws1")), Some("aws1"), "aws"), Some(Owner::Current));
        assert_eq!(reap_owner(Some(("aws", "aws1")), None, "aws"), Some(Owner::Current), "a state from before hosts were recorded");
        // The lab moved to Proxmox (or this machine): the AWS state is a leftover of aws1.
        assert_eq!(reap_owner(Some(("proxmox", "pve1")), Some("aws1"), "aws"), Some(Owner::Left("aws1".into())));
        assert_eq!(reap_owner(None, Some("aws1"), "aws"), Some(Owner::Left("aws1".into())));
        assert_eq!(reap_owner(Some(("aws", "aws2")), Some("aws1"), "aws"), Some(Owner::Left("aws1".into())));
        // Nothing says which credentials can destroy it.
        assert_eq!(reap_owner(None, None, "aws"), None);
        assert_eq!(reap_owner(Some(("proxmox", "pve1")), None, "aws"), None);
    }
    use crate::runtime::test_support::status_with;
    use crate::runtime::{Runtime, server};
    use crate::test_app::TestApp;

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
        stop_lab(t.handle(), "web", Runtime::Docker).await.unwrap();
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
        resume_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.unwrap();
        assert!(tools.called("vagrant up --no-provision --provider virtualbox"), "{:?}", tools.calls());
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        stop_lab(t.handle(), "ad", Runtime::Vm).await.unwrap();
        assert!(tools.called("vagrant destroy --force"));
        assert_eq!(parked(&dir), None);
    }

    #[tokio::test]
    async fn a_failed_park_or_stop_keeps_the_labs_marks() {
        let t = TestApp::new();
        let dir = t.lab("ad", &[]);
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        let _tools = Tools::rules(&[("vagrant up", fail("VBoxManage: error")), ("vagrant halt", fail("locked")), ("destroy", fail("locked"))]);
        assert!(resume_lab(t.handle(), "ad", Runtime::Vm, |_| {}).await.is_err());
        assert_eq!(parked(&dir), Some(Park::Pause));
        assert!(park_lab(t.handle(), "ad", Runtime::Vm, Park::Shutdown, |_| {}).await.is_err());
        assert_eq!(parked(&dir), Some(Park::Pause));
        assert!(stop_lab(t.handle(), "ad", Runtime::Vm).await.is_err());
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
        stop_lab(t.handle(), "web", Runtime::Docker).await.unwrap();
        assert!(!dir.join(server::HOST_MARKER).exists());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn the_reaper_only_looks_at_expired_cloud_labs() {
        let t = TestApp::new();
        let tools = Tools::rules(&[]);
        reap_expired_labs(t.handle()).await;
        // A deployment whose lab isn't installed, one on no host, one not expired yet.
        for id in ["gone", "here", "fresh"] {
            std::fs::create_dir_all(t.data().join("deployments").join(id).join("azure")).unwrap();
        }
        t.lab("here", &[]);
        let fresh = t.lab("fresh", &[]);
        server::mark_lab(&fresh, Some(&t.host("az1", "azure", serde_json::json!({})))).unwrap();
        reap_expired_labs(t.handle()).await;
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        // Expired, with billable resources: torn down (best effort; Terraform's module is missing).
        let state = t.data().join("deployments").join("fresh").join("azure");
        std::fs::write(state.join("run.json"), r#"{"expires_at": 1}"#).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"resources": [{"type": "azurerm_linux_virtual_machine"}]}"#).unwrap();
        reap_expired_labs(t.handle()).await;
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
