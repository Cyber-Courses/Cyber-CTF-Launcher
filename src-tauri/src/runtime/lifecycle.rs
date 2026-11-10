//! A lab after it started: parking (pause, shut down) and resuming it, re-running provisioning,
//! and stopping it wherever it runs (the auto-stop reaper, in `reaper`, stops expired ones).

use std::path::Path;

use crate::platform::AppDirs;

use super::host::{self, Host};
use super::inflight::{Action, DeployGuard, lock_lab};
use super::launch::policy_allows;
use super::paths::{lab_dir, local_runtime, local_vm, mark_engine, mark_local_vm, mark_parked, mark_per_machine_vm, parked, per_machine_vm};
use super::{Control, LabRef, Park, Runtime, isothrum, lab, server};
use crate::error::{Error, Result};

/// Parks a lab running on this machine: its machines are kept and resume as they were, so a lab
/// that took an hour to build comes back in seconds (paused) or a boot (shut down), not a rebuild.
async fn park(dirs: &AppDirs, lab: LabRef<'_>, mode: Park, mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    let _lock = lock_lab(dirs, id).await?;
    let _deploy = DeployGuard::new(id, Action::Park);
    refuse_cloud_park(runtime)?;
    // In the form it was started in: a container lab run as one VM per machine parks as a VM lab.
    let run = local_run(dirs, dir, id, runtime, "Pausing a lab on a server host isn't available yet. Stop it instead.")?;
    // A container lab in one local VM is paused (its state saved), never shut down.
    if !shuts_down(run.target()) {
        refuse_vm_shutdown(mode)?;
    }
    run.park(mode, &mut log).await?;
    mark_parked(dir, Some(mode))?;
    log(parked_message(mode).into());
    Ok(())
}

/// Lab `id`'s run on this machine ([`host::locate`]), for a lab parked or resumed here; refused
/// with `elsewhere` for a lab on a server host or cloud account. A cloud-services lab, refused
/// before, has none.
fn local_run(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime, elsewhere: &str) -> Result<isothrum::LabRun> {
    match host::locate(dirs, dir, id, runtime)? {
        Some(Host::Local(run)) => Ok(run),
        Some(_) => Err(Error::Invalid(elsewhere.into())),
        None => Err(Error::Invalid(CLOUD_PARK.into())),
    }
}

/// Cloud services are created or destroyed, never kept switched off.
fn refuse_cloud_park(runtime: Runtime) -> Result<()> {
    if runtime == Runtime::Cloud {
        return Err(Error::Invalid(CLOUD_PARK.into()));
    }
    Ok(())
}

/// Why a cloud-services lab isn't parked.
const CLOUD_PARK: &str = "A cloud lab can't be paused: stop it to end what it costs, and start it again later.";

/// Inside the VM the containers don't restart on their own after a power-off, so a container
/// lab in a VM is paused (its state saved), never shut down.
fn refuse_vm_shutdown(mode: Park) -> Result<()> {
    if mode == Park::Shutdown {
        return Err(Error::Invalid("A container lab running in a VM can be paused, not shut down.".into()));
    }
    Ok(())
}

/// Whether a run on this machine for `target` can be shut down: not a container lab in one
/// VM ([`refuse_vm_shutdown`]).
fn shuts_down(target: lab::Target) -> bool {
    target != lab::Target::DockerVm
}

/// Whether a run on this machine for `target`, on `provider`, can be paused: its VMs suspend
/// (a container lab here only stops its containers: there is no state to save, so it is shut
/// down), unless the provider has no suspend (vagrant-qemu answers "this provider doesn't
/// support this action").
fn pauses(target: lab::Target, provider: Option<&str>) -> bool {
    target != lab::Target::Docker && provider != Some("qemu")
}

/// What can be done on a lab where it runs (`host`, on `provider` as its status reads it): pause
/// and shut down only here ([`park`] refuses elsewhere, and a cloud-services lab, which locates
/// no run here, is never parked); provisioning on a lab run as VMs through Vagrant, here or on an
/// ESXi host ([`provision`]); bringing machines that went down back ([`resume`], which runs here
/// only) on a lab run as VMs here, a container lab run as one VM per machine included. None
/// where nothing runs.
pub(crate) fn controls(host: Option<&Host>, provider: Option<&str>) -> Vec<Control> {
    match host {
        Some(Host::Local(run)) => {
            let (target, vms) = (run.target(), run.target() == lab::Target::Vagrant);
            [(pauses(target, provider), Control::Pause), (shuts_down(target), Control::Shutdown), (vms, Control::Provision), (vms, Control::Repair)]
                .into_iter()
                .filter_map(|(offered, c)| offered.then_some(c))
                .collect()
        }
        Some(Host::Esxi { run, .. }) if run.target() == lab::Target::Vagrant => vec![Control::Provision],
        Some(_) | None => Vec::new(),
    }
}

/// The last line of a park.
fn parked_message(mode: Park) -> &'static str {
    match mode {
        Park::Pause => "Lab paused. Resume it to pick up where you left off.",
        Park::Shutdown => "Lab shut down. Resume it to boot its machines again.",
    }
}

/// Brings a parked lab back, as it was. Also the repair for a lab some machines of which went
/// down (powered off by hand, or a cut resume): the machines already up are left alone, the others
/// come back. A resume boots the lab's machines again, so the administrator's policy is asked
/// first, as for a start ([`policy_allows`]).
async fn resume(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime, mut log: impl FnMut(String)) -> Result<()> {
    let _lock = lock_lab(dirs, id).await?;
    let _deploy = DeployGuard::new(id, Action::Start);
    refuse_cloud_park(runtime)?;
    policy_allows(dirs, runtime, None, || Ok(lab::is_privileged(&lab::load(dir)?)))?;
    let run = local_run(dirs, dir, id, runtime, "This lab was parked on a server host; resuming there isn't available yet.")?;
    log("Resuming the lab…".into());
    // The runner resumes a parked run as it was; when nothing is left of it, it drops the parked
    // mark and reports `NothingToResume`, so the page offers a fresh start.
    run.resume(&mut log).await?;
    mark_parked(dir, None)?;
    log("Lab resumed.".into());
    Ok(())
}

/// Runs a VM lab's provisioners again on its running machines (one, or all): the repair for a
/// machine whose provisioning was cut short, without rebuilding the lab. Inputs the lab declared
/// (`isoloom.yml` `inputs`) were given at launch and aren't kept, so a lab that provisions from
/// them gets none here; the labs so far declare none.
async fn provision(dirs: &AppDirs, lab: LabRef<'_>, machine: Option<&str>, mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    // A container lab run as one VM per machine has VMs to provision, like a VM lab.
    let runtime = local_runtime(dir, runtime);
    if runtime != Runtime::Vm {
        return Err(Error::Invalid("Container labs aren't provisioned in place: use Reset for a clean copy.".into()));
    }
    let _lock = lock_lab(dirs, id).await?;
    let _deploy = DeployGuard::new(id, Action::Start);
    // Through the runner: a VM lab on this machine, or on an ESXi host. Proxmox and the clouds
    // aren't re-provisioned in place yet.
    let Some(Host::Local(run) | Host::Esxi { run, .. }) = host::locate(dirs, dir, id, runtime)? else {
        return Err(Error::Invalid("Re-running provisioning isn't available for labs on Proxmox or a cloud yet.".into()));
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
async fn stop(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let _lock = lock_lab(dirs, id).await?;
    stop_locked(dirs, dir, id, runtime, log).await
}

/// The body of `stop`, for a caller already holding the lab lock (the lock is not reentrant, so
/// it must not call `stop`, which would deadlock): the integration tests.
pub(crate) async fn stop_locked(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let _deploy = DeployGuard::new(id, Action::Stop);
    // Wherever it runs, through the runner (its attack box, a machine of it, goes with it): on
    // this machine, or on a server host or cloud account, destroyed there with the parameters it
    // was applied with and the connection that works now. A cloud-services lab on no account has
    // nothing anywhere: its marks are cleared.
    let mut log = log;
    if let Some(host) = host::locate(dirs, dir, id, runtime)? {
        host.down(dirs, &mut log).await?;
    }
    forget_run(dir)?;
    // What the runner hasn't taken over yet, nothing here removes: said, never reported stopped.
    match super::status::left_awaiting_hand_over(dirs, id)? {
        Some(left) => Err(left),
        None => Ok(()),
    }
}

/// Destroys a lab's run on this machine (`run`, of the lab in `dir`) and, once it is gone, clears
/// what marked it as running. Takes no app handle, so the integration tests run exactly this.
pub(crate) async fn stop_run(run: isothrum::LabRun, dir: &Path, mut log: impl FnMut(String)) -> Result<()> {
    run.down(&mut log).await?;
    forget_run(dir)
}

/// A stopped lab's markers: where it ran, its local VM, its per-machine form, its engine, parked;
/// and the attack box the launcher added to it (the next start is the lab as published).
fn forget_run(dir: &Path) -> Result<()> {
    super::attack_machine::remove(dir)?;
    server::mark_lab(dir, None)?;
    mark_local_vm(dir, None)?;
    mark_per_machine_vm(dir, false)?;
    mark_engine(dir, None)?;
    mark_parked(dir, None)
}

/// Takes down a parked lab before its folder is replaced by a newer version of the lab: its
/// machines would otherwise be orphaned (the runner's record of them goes with the folder).
/// Through the runner, in the form it was parked in ([`host::locate`]), its marks cleared after;
/// nothing to do when it isn't parked.
pub async fn clear_parked(dirs: &AppDirs, lab: LabRef<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    if parked(dir).is_none() {
        return Ok(());
    }
    log("A parked copy of an older version of this lab is here: removing it…".into());
    if let Some(host) = host::locate(dirs, dir, id, runtime)? {
        host.down(dirs, log).await?;
    }
    forget_run(dir)
}

// The bodies of the lab operations the deploy worker runs (`deploy_worker::execute`), by lab id.

pub async fn park_lab(dirs: &AppDirs, id: &str, runtime: Runtime, mode: Park, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    park(dirs, LabRef { dir: &dir, id, runtime }, mode, log).await
}

pub async fn resume_lab(dirs: &AppDirs, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    resume(dirs, &lab_dir(dirs, id)?, id, runtime, log).await
}

pub async fn provision_lab(dirs: &AppDirs, id: &str, runtime: Runtime, machine: Option<&str>, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    provision(dirs, LabRef { dir: &dir, id, runtime }, machine, log).await
}

/// How lab `id` runs on this machine, for a Stop of what the Machine page found of it there
/// (`in_vm`: its VMs, else its containers). `None` when it isn't an installed lab that runs here
/// (removed since, the self-test, or marked as running on a server host): its leftovers then go
/// by name.
pub fn runtime_here(dirs: &AppDirs, id: &str, in_vm: bool) -> Option<Runtime> {
    let dir = lab_dir(dirs, id).ok()?;
    if server::marked_host(&dir).is_some_and(|h| !h.is_empty()) {
        return None;
    }
    // Its VMs are its machines (a VM lab, or a container lab run as one VM per machine), unless
    // the one VM is the helper a container lab runs inside.
    Some(if per_machine_vm(&dir) || (in_vm && local_vm(&dir).is_none()) { Runtime::Vm } else { Runtime::Docker })
}

/// Stops lab `id` wherever it runs: the one Stop of the lab page, the Machine page and the
/// hosted agent (through `labs::stop_lab`). Marked as stopping from the start: `end_first` (what
/// the layers above end with the lab: its deploy workers, a deploy still running in one of which
/// can take minutes while Terraform is interrupted and waited for, and the website's co-location
/// check, which has nothing left to vouch for) runs first, while the page and the quit guard see
/// a stop and no attack box may be added, so the teardown never races a `vagrant up` or `compose
/// up` that would recreate what it removes; then the lab is torn down under its lock.
pub async fn stop_lab(dirs: &AppDirs, id: &str, runtime: Runtime, end_first: impl Future<Output = ()>, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    let _stopping = DeployGuard::new(id, Action::Stop);
    end_first.await;
    stop(dirs, &dir, id, runtime, log).await
}

#[cfg(test)]
mod tests {
    use super::{
        LabRef, Park, clear_parked, park_lab, parked_message, provision_lab, provision_target, refuse_cloud_park, refuse_vm_shutdown, resume_lab, stop_lab,
    };
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
        let err = park_lab(&t.dirs(), "web", Runtime::Docker, Park::Pause, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("Pausing a lab on a server host"), "{err}");
        let err = resume_lab(&t.dirs(), "web", Runtime::Docker, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("parked on a server host"), "{err}");
        let err = provision_lab(&t.dirs(), "web", Runtime::Vm, None, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("Proxmox or a cloud"), "{err}");
        assert_eq!(tools.calls(), Vec::<String>::new());
        // Stopping it destroys it with Terraform there: with no state, nothing is left to destroy,
        // and the lab no longer reads as running there.
        stop_lab(&t.dirs(), "web", Runtime::Docker, std::future::ready(()), |_| {}).await.unwrap();
        assert!(!dir.join(server::HOST_MARKER).exists());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_stop_says_what_it_cant_remove_of_a_deployment_the_runner_hasnt_taken_over() {
        // Regression: the Stop went through, the deployment of the launcher's own Terraform path
        // the runner couldn't take stayed up (nothing runs Terraform for it any more), and the
        // player had been told Stop would remove it.
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        let host = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true, "host": "us-east-1" }));
        server::mark_lab(&dir, Some(&host)).unwrap();
        let state = t.data().join("deployments/web/aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[{"type":"aws_instance"}]}"#).unwrap();
        std::fs::write(state.join("host"), &host).unwrap();
        let tools = Tools::rules(&[]);
        let err = stop_lab(&t.dirs(), "web", Runtime::Docker, std::future::ready(()), |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("the launcher can't remove") && err.contains("delete its resources in your AWS console"), "{err}");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        // Once the runner took it over (no resources left in the old state), the Stop goes through.
        std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[]}"#).unwrap();
        stop_lab(&t.dirs(), "web", Runtime::Docker, std::future::ready(()), |_| {}).await.unwrap();
    }

    #[tokio::test]
    async fn the_policy_refuses_resuming_a_privileged_lab_before_anything_runs() {
        use crate::error::Code;
        use crate::platform::policy::{Key, Override};
        let t = TestApp::new();
        let dir = t.lab("priv", &[("isoloom.yml", crate::runtime::test_support::PRIVILEGED_LAB)]);
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        let tools = Tools::rules(&[]);
        let _policy = Override::with(&[Key::RefusePrivilegedLabs]);
        let err = resume_lab(&t.dirs(), "priv", Runtime::Docker, |_| {}).await.unwrap_err();
        assert_eq!(err.code(), Code::TurnedOffByPolicy { setting: "RefusePrivilegedLabs".into() }, "{err}");
        assert_eq!(parked(&dir), Some(Park::Pause));
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_parked_lab_is_taken_down_through_the_runner_before_its_folder_is_replaced() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        mark_parked(&dir, Some(Park::Shutdown)).unwrap();
        crate::runtime::paths::mark_engine(&dir, Some("orbstack")).unwrap();
        let tools = Tools::rules(&[]);
        let mut lines = Vec::<String>::new();
        let lab = LabRef { dir: &dir, id: "web", runtime: Runtime::Docker };
        clear_parked(&t.dirs(), lab, &mut |l| lines.push(l)).await.unwrap();
        // No files of a run left: nothing to remove, and its marks go.
        assert_eq!(parked(&dir), None);
        assert_eq!(crate::runtime::paths::engine(&dir), None);
        assert!(lines[0].starts_with("A parked copy of an older version"), "{lines:?}");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_parked_lab_that_couldnt_be_taken_down_stays_parked_and_says_why() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        // The runner can't read the run's record: the teardown fails.
        let manifest = crate::runtime::lab::docker_manifest(&dir);
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, "not a manifest").unwrap();
        let _tools = Tools::rules(&[]);
        let lab = LabRef { dir: &dir, id: "web", runtime: Runtime::Docker };
        assert!(clear_parked(&t.dirs(), lab, &mut |_| {}).await.is_err());
        assert_eq!(parked(&dir), Some(Park::Pause), "still parked: its machines may still be there");
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
        stop_lab(&t.dirs(), "goat", Runtime::Cloud, std::future::ready(()), |_| {}).await.unwrap();
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert_eq!(parked(&dir), None);
        // Parking or resuming it is refused on the runtime alone.
        let err = park_lab(&t.dirs(), "goat", Runtime::Cloud, Park::Pause, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
        let err = resume_lab(&t.dirs(), "goat", Runtime::Cloud, |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
        assert_eq!(tools.calls(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn clearing_a_lab_that_is_not_parked_does_nothing() {
        let t = TestApp::new();
        let dir = t.lab("web-1", &[]);
        let tools = Tools::rules(&[]);
        let mut lines = Vec::<String>::new();
        clear_parked(&t.dirs(), LabRef { dir: &dir, id: "web-1", runtime: Runtime::Vm }, &mut |l| lines.push(l)).await.unwrap();
        assert_eq!(lines, [] as [String; 0]);
        assert_eq!(tools.calls(), [] as [String; 0]);
    }

    /// What the page may do on lab `t`, declared `runtime`, where it is located now, on `provider`:
    /// whether it pauses, shuts down, provisions and repairs.
    fn controls_of(t: &TestApp, dir: &std::path::Path, runtime: Runtime, provider: Option<&str>) -> (bool, bool, bool, bool) {
        use crate::runtime::Control;
        let host = crate::runtime::host::locate(&t.dirs(), dir, "t", runtime).unwrap();
        let c = super::controls(host.as_ref(), provider);
        let has = |x| c.contains(&x);
        (has(Control::Pause), has(Control::Shutdown), has(Control::Provision), has(Control::Repair))
    }

    #[test]
    fn the_page_is_offered_what_the_lifecycle_accepts_where_the_lab_runs() {
        use crate::runtime::paths::{mark_local_vm, mark_per_machine_vm};
        let t = TestApp::new();
        let _tools = Tools::rules(&[]);
        let spec = "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, os: debian@12, networks: { lab: 10 } }\n";
        let dir = t.lab("t", &[("isoloom.yml", spec)]);
        // Containers here: shut down only (nothing to save), no provisioning.
        assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("docker")), (false, true, false, false));
        // A VM lab here: both, provisioning and the repair; QEMU can't suspend.
        assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("virtualbox")), (true, true, true, true));
        assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("qemu")), (false, true, true, true));
        // A container lab run as one VM per machine is a VM lab here, repaired as one too.
        mark_per_machine_vm(&dir, true).unwrap();
        assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("virtualbox")), (true, true, true, true));
        mark_per_machine_vm(&dir, false).unwrap();
        // Containers in one local VM: paused, never shut down, provisioned nor repaired.
        mark_local_vm(&dir, Some(crate::runtime::providers::Provider::Virtualbox)).unwrap();
        assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("virtualbox")), (true, false, false, false));
        mark_local_vm(&dir, None).unwrap();
        // A cloud-services lab on no account: nothing runs.
        assert_eq!(controls_of(&t, &dir, Runtime::Cloud, None), (false, false, false, false));
        // In a cloud account, or on Proxmox: nothing parked or provisioned there.
        server::mark_lab(&dir, Some(&t.host("acct", "aws", serde_json::json!({ "useCliCreds": true })))).unwrap();
        assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("aws")), (false, false, false, false));
        crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
        for host in ["pve", "esx"] {
            crate::runtime::server::secrets::set_secret(host, "pw").unwrap();
        }
        server::mark_lab(&dir, Some(&t.host("pve", "proxmox", serde_json::json!({})))).unwrap();
        assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("proxmox")), (false, false, false, false));
        // On ESXi: provisioning a VM lab, through Vagrant there; never its containers' VM.
        server::mark_lab(&dir, Some(&t.host("esx", "vmware_esxi", serde_json::json!({})))).unwrap();
        assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("vmware_esxi")), (false, false, true, false));
        assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("vmware_esxi")), (false, false, false, false));
    }
}
