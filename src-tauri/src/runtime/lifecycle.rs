//! A lab after it started: parking (pause, shut down) and resuming it, re-running provisioning,
//! and stopping it wherever it runs (the auto-stop reaper, in `reaper`, stops expired ones).

use std::path::Path;

use crate::AppHandle;

use super::inflight::{Action, DeployGuard, lock_lab};
use super::paths::{lab_dir, local_vm, mark_local_vm, mark_parked, parked, runner_home, vagrant_dirs};
use super::{Park, Runtime, docker, isothrum, lab, server, vm};
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
        // A container lab in one local VM is paused (its state saved), never shut down.
        Runtime::Docker if local_vm(dir).is_some() => {
            refuse_vm_shutdown(mode)?;
            isothrum::LabRun::docker_in_vm(&runner_home(app)?, dir, id)?.park(mode, &mut log).await?;
        }
        Runtime::Docker => isothrum::LabRun::docker(&runner_home(app)?, dir, id)?.park(mode, &mut log).await?,
        Runtime::Vm => isothrum::LabRun::vm(&runner_home(app)?, dir, id)?.park(mode, &mut log).await?,
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
    // The runner resumes a parked run as it was; when nothing is left of it, it drops the parked
    // mark and reports `NothingToResume`, so the page offers a fresh start.
    match runtime {
        Runtime::Docker if local_vm(dir).is_some() => isothrum::LabRun::docker_in_vm(&runner_home(app)?, dir, id)?.resume(&mut *log).await?,
        Runtime::Docker => isothrum::LabRun::docker(&runner_home(app)?, dir, id)?.resume(&mut *log).await?,
        Runtime::Vm => isothrum::LabRun::vm(&runner_home(app)?, dir, id)?.resume(&mut *log).await?,
        Runtime::Cloud => unreachable!("refused above"),
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
    let _lock = lock_lab(app, id).await?;
    let _deploy = DeployGuard::new(id, Action::Start);
    // Through the runner: a VM lab on this machine, or on an ESXi host. Proxmox and the clouds
    // aren't re-provisioned in place yet.
    let run = match server::lab_connection(app, dir)? {
        None => isothrum::LabRun::vm(&runner_home(app)?, dir, id)?,
        Some(c) if c.provider == super::providers::Provider::VmwareEsxi => super::launch::esxi_run(app, dir, id, runtime, &c)?,
        Some(_) => return Err(Error::Invalid("Re-running provisioning isn't available for labs on Proxmox or a cloud yet.".into())),
    };
    log(provision_target(&run.status().await?, machine)?);
    run.provision(machine, &mut log).await?;
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
    // On this machine, through the runner: a container lab on Docker or in one local VM, or a VM
    // lab (its attack VM goes with it). Cloud-services have a host, so they never reach here.
    let Some(c) = server::lab_connection(app, dir)? else {
        return match runtime {
            Runtime::Docker if local_vm(dir).is_some() => stop_docker_in_vm_run(&runner_home(app)?, dir, id, log).await,
            Runtime::Docker => stop_docker_run(&runner_home(app)?, dir, id, log).await,
            Runtime::Vm => stop_vm_run(&runner_home(app)?, dir, id, log).await,
            Runtime::Cloud => stop_here(dir, id, runtime, log).await,
        };
    };
    // On a server host or cloud account: destroyed through the runner, which replays the
    // parameters it was applied with; the connection given here is the one that works now.
    let mut log = log;
    let result = if c.provider == super::providers::Provider::VmwareEsxi {
        stop_esxi_run(app, dir, id, runtime, &c, &mut log).await
    } else if c.provider.is_cloud() {
        stop_cloud_run(app, CloudStop { dir, id, runtime, conn: &c }, &mut log).await
    } else {
        stop_server_run(app, dir, id, runtime, &c, &mut log).await
    };
    if result.is_ok() {
        forget_run(dir)?;
    }
    result
}

/// Destroys a container lab that ran in one local VM, through the runner (its containers go with
/// the VM), and clears what marked it as running.
pub(super) async fn stop_docker_in_vm_run(home: &std::path::Path, dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    isothrum::LabRun::docker_in_vm(home, dir, id)?.down(&mut log).await?;
    forget_run(dir)
}

/// Destroys a Proxmox lab the runner runs (connection `c`, the server's as it is now).
async fn stop_server_run(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, c: &server::Connection, log: &mut impl FnMut(String)) -> Result<()> {
    let host = server::marked_host(dir).unwrap_or_default();
    let run = isothrum::ServerRun::new(&runner_home(app)?, dir, id, runtime)?;
    let place = super::launch::server_place(app, &host, c, &run).await?;
    run.down(&place, log).await
}

/// Destroys an ESXi lab the runner runs (connection `c`, the server's as it is now).
async fn stop_esxi_run(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, c: &server::Connection, log: &mut impl FnMut(String)) -> Result<()> {
    super::launch::esxi_run(app, dir, id, runtime, c)?.down(log).await
}

/// A cloud VM lab to tear down: the lab (folder, id, runtime) and the account it ran on.
struct CloudStop<'a> {
    dir: &'a Path,
    id: &'a str,
    runtime: Runtime,
    conn: &'a server::Connection,
}

/// Destroys a cloud VM lab in its account (as it is now), through the runner, which replays the
/// recorded parameters with the account's credentials.
async fn stop_cloud_run(app: &AppHandle, stop: CloudStop<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let CloudStop { dir, id, runtime, conn: c } = stop;
    let cloud = server::terraform_target(c.provider).unwrap_or_default();
    let label = server::marked_host(dir).unwrap_or_default();
    let place = isothrum::CloudPlace { label, region: c.region.clone(), tf_env: c.tf_env.clone(), ..isothrum::CloudPlace::default() };
    isothrum::CloudRun::new(&runner_home(app)?, dir, id, runtime, cloud)?.down(&place, log).await
}

/// Stops a lab that runs on this machine (its containers, a local VM, or its VMs) and, once it
/// is gone, clears what marked it as running. Takes the lab folder (no app handle), so the
/// real-lab tests run exactly this.
pub(super) async fn stop_here(dir: &Path, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let result = match runtime {
        Runtime::Docker if local_vm(dir).is_some() => vm::stop(&lab::vagrant_dir(dir, runtime), &[], log).await,
        Runtime::Docker => docker::stop(dir, id, log).await,
        // The on-demand attack box is part of the run (a machine of the manifest), so it goes
        // with the lab's VMs here.
        Runtime::Vm => vm::stop(&lab::vagrant_dir(dir, runtime), &[], log).await,
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

/// Stops a VM lab the Isothrum runner runs (runner home `home`) — its on-demand attack box, a
/// machine of the run, goes with it — and once it is gone clears what marked it as running. Takes
/// no app handle, so the real-lab tests run exactly this.
pub(super) async fn stop_vm_run(home: &std::path::Path, dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    isothrum::LabRun::vm(home, dir, id)?.down(&mut log).await?;
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
    use crate::platform::exec::fake::Tools;
    use crate::runtime::paths::{mark_parked, parked};

    use crate::runtime::test_support::status_with;
    use crate::runtime::{Runtime, server};
    use crate::testing::test_app::TestApp;

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
