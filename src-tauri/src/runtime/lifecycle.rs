//! A lab after it started: parking (pause, shut down) and resuming it, re-running provisioning,
//! stopping it wherever it runs, and the reaper that destroys expired cloud labs.

use std::path::Path;

use tauri::{AppHandle, Manager};

use super::inflight::{Action, DeployGuard, lock_lab, try_lock_lab};
use super::paths::{deployed_ids, lab_dir, local_vm, mark_local_vm, mark_parked, parked, recorded_runtime, state_dir, vagrant_dirs};
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
pub async fn reap_expired_labs(app: &AppHandle) {
    let Ok(base) = app.path().app_data_dir() else { return };
    for id in deployed_ids(&base) {
        let Ok(dir) = lab_dir(app, &id) else { continue };
        let Ok(Some(c)) = server::lab_connection(app, &dir) else { continue };
        let Some(target) = server::terraform_target(c.provider) else { continue };
        let Ok(state) = state_dir(app, &id, target) else { continue };
        if !terraform::expired(&state) {
            continue;
        }
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
        let _ = stop_locked(app, &dir, &id, recorded_runtime(&state), |_line: String| {}).await;
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
    use super::{Park, clear_parked, parked_message, provision_target, refuse_vm_shutdown};
    use crate::runtime::test_support::status_with;

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
