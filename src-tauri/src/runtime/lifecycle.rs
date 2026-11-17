//! A lab after it started: parking (pause, shut down) and resuming it, re-running provisioning,
//! and stopping it wherever it runs (the auto-stop reaper, in `reaper`, stops expired ones).

use std::path::Path;

use crate::platform::AppDirs;

use super::host::{self, Host};
use super::launch::policy_allows;
use super::paths::lab_dir;
use super::recorded::Recorded;
use super::{Control, LabRef, Park, Runtime, isothrum, lab};
use crate::error::{Error, Result};

/// Parks a lab running on this machine: its machines are kept and resume as they were, so a lab
/// that took an hour to build comes back in seconds (paused) or a boot (shut down), not a rebuild.
async fn park(dirs: &AppDirs, lab: LabRef<'_>, mode: Park, mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { runtime, .. } = lab;
    refuse_cloud_park(runtime)?;
    // In the form it was started in: a container lab run as one VM per machine parks as a VM lab.
    let run = local_run(dirs, lab, "Pausing a lab on a server or a cloud account isn't available. Stop it instead.").await?;
    // Only what the page is offered for this run ([`controls`]): QEMU refuses a pause.
    if let Some(why) = park_refusal(mode, run.target(), &run.parks().await?) {
        return Err(Error::Refused(why.into()));
    }
    run.park(mode, &mut log).await?;
    log(parked_message(mode).into());
    Ok(())
}

/// Lab `id`'s run on this machine ([`host::locate`]), for a lab parked or resumed here; refused
/// with `elsewhere` for a lab on a server host or cloud account. A cloud-services lab, refused
/// before, has none.
async fn local_run(dirs: &AppDirs, lab: LabRef<'_>, elsewhere: &str) -> Result<isothrum::Deployment> {
    let LabRef { dir, id, runtime } = lab;
    match host::locate(dirs, dir, id, runtime).await? {
        Some(Host { run, conn: None }) => Ok(run),
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

/// Why a run on this machine for `target`, parked in the ways `parks`, can't be parked in `mode`: the [`local_controls`] it is offered don't hold
/// that mode's control. None when they do.
fn park_refusal(mode: Park, target: Option<lab::Target>, parks: &[Park]) -> Option<&'static str> {
    let control = match mode {
        Park::Pause => Control::Pause,
        Park::Shutdown => Control::Shutdown,
    };
    if local_controls(target, parks).contains(&control) {
        return None;
    }
    // Every form here shuts down: only a pause on QEMU (no suspend) is refused.
    Some("QEMU can't pause a lab: shut it down instead.")
}

/// What can be done on a lab where it runs (`host`, parked in the ways `parks` as its status
/// reads them): pause and shut down only here ([`park`] refuses elsewhere, and a cloud-services lab, which locates
/// no run here, is never parked); provisioning on a lab run as VMs through Vagrant, here or on an
/// ESXi host ([`provision`]); bringing machines that went down back ([`resume`], which runs here
/// only) on a lab run as VMs here, a container lab run as one VM per machine included, and on a
/// container lab on Docker here (Compose starts its stopped containers again). None where nothing
/// runs.
pub(crate) fn controls(host: Option<&Host>, parks: &[Park]) -> Vec<Control> {
    match host {
        Some(Host { run, conn: None }) => local_controls(run.target(), parks),
        Some(host) if host.on_esxi() && host.target() == Some(lab::Target::Vagrant) => vec![Control::Provision],
        Some(_) | None => Vec::new(),
    }
}

/// [`controls`] for a run on this machine for `target`, parked in the ways `parks`: the one rule
/// [`park`] holds too. Its containers freeze (`compose pause`) and its VMs suspend, unless the
/// provider has no suspend (vagrant-qemu); every form shuts down. None for a run with no target
/// (a module of the lab's own, which never runs here).
fn local_controls(target: Option<lab::Target>, parks: &[Park]) -> Vec<Control> {
    let Some(target) = target else { return Vec::new() };
    let vms = target == lab::Target::Vagrant;
    // Containers stopped outside the app (`docker stop`, Docker Desktop's Stop) come back with a
    // resume as VMs do; one removed can't, and the page offers its reset instead.
    let repairs = vms || target == lab::Target::Docker;
    let parks = |mode| parks.contains(&mode);
    [(parks(Park::Pause), Control::Pause), (parks(Park::Shutdown), Control::Shutdown), (vms, Control::Provision), (repairs, Control::Repair)]
        .into_iter()
        .filter_map(|(offered, c)| offered.then_some(c))
        .collect()
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
async fn resume(dirs: &AppDirs, lab: LabRef<'_>, mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, runtime, .. } = lab;
    refuse_cloud_park(runtime)?;
    policy_allows(dirs, runtime, None, || Ok(lab::is_privileged(&lab::load(dir)?)))?;
    let run = local_run(dirs, lab, "A lab on a server or a cloud account isn't parked, so there is nothing to resume: start it again.").await?;
    log("Resuming the lab…".into());
    // The runner resumes a parked run as it was; when nothing is left of it, it reports
    // `NothingToResume` and its record reads absent, so the page offers a fresh start.
    run.resume(&mut log).await?;
    log("Lab resumed.".into());
    Ok(())
}

/// Runs a VM lab's provisioners again on its running machines (one, or all): the repair for a
/// machine whose provisioning was cut short, without rebuilding the lab. Inputs the lab declared
/// (`isoloom.yml` `inputs`) get the values the run was launched with (`launch_values`).
async fn provision(dirs: &AppDirs, lab: LabRef<'_>, machine: Option<&str>, mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    // A container lab run as one VM per machine has VMs to provision, like a VM lab.
    let runtime = Recorded::of(dirs, id).await?.form(runtime);
    if runtime != Runtime::Vm {
        return Err(Error::Invalid("Container labs aren't provisioned in place: use Reset for a clean copy.".into()));
    }
    // Through the runner: a VM lab on this machine, or on an ESXi host. Proxmox and the clouds
    // aren't re-provisioned in place yet.
    let Some(Host { run, .. }) = host::locate(dirs, dir, id, runtime).await?.filter(|h| h.is_here() || h.on_esxi()) else {
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
pub(crate) async fn stop(dirs: &AppDirs, lab: LabRef<'_>, log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    // Wherever it runs, through the runner (its attack box, a machine of it, goes with it): on
    // this machine, or on a server host or cloud account, destroyed there with the parameters it
    // was applied with and the connection that works now. A cloud-services lab on no account has
    // nothing anywhere: its marks are cleared.
    let mut log = log;
    if let Some(host) = host::locate(dirs, dir, id, runtime).await? {
        host.down(dirs, &mut log).await?;
    }
    forget_run(dir)
}

/// Destroys a lab's run on this machine (`run`, of the lab in `dir`) and, once it is gone, clears
/// what marked it as running. Takes no app handle, so the integration tests run exactly this.
#[cfg(test)]
pub(crate) async fn stop_run(run: isothrum::Deployment, dir: &Path, mut log: impl FnMut(String)) -> Result<()> {
    run.down(&mut log).await?;
    forget_run(dir)
}

/// What a stopped lab leaves behind of its run in its folder: the attack box the launcher added
/// to it (the next start is the lab as published). Where it ran, on which engine, in what form and
/// parked or not, the runner's record says (absent now).
fn forget_run(dir: &Path) -> Result<()> {
    super::attack_box::remove(dir)
}

/// Takes down a parked lab before its folder is replaced by a newer version of the lab: its
/// machines would otherwise be orphaned (its generated files go with the folder). Through the
/// runner, in the form it was parked in ([`host::locate`]); nothing to do when its run isn't
/// recorded parked.
pub async fn clear_parked(dirs: &AppDirs, lab: LabRef<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    if Recorded::of(dirs, id).await?.parked().is_none() {
        return Ok(());
    }
    log("A parked copy of an older version of this lab is here: removing it…".into());
    if let Some(host) = host::locate(dirs, dir, id, runtime).await? {
        host.down(dirs, log).await?;
    }
    forget_run(dir)
}

// The bodies of the lab operations the run service runs in the app (`run_service::local`), by lab id.

/// Parks lab `id`, declared `runtime` (pauses it or shuts it down, by `mode`), its progress lines
/// to `log`.
pub async fn park_lab(dirs: &AppDirs, id: &str, runtime: Runtime, mode: Park, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    park(dirs, LabRef { dir: &dir, id, runtime }, mode, log).await
}

/// Brings parked lab `id`, declared `runtime`, back up as it was, its progress lines to `log`.
pub async fn resume_lab(dirs: &AppDirs, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    resume(dirs, LabRef { dir: &dir, id, runtime }, log).await
}

/// Runs the provisioning of lab `id`, declared `runtime`, again in place: of `machine` alone when
/// given, else of every machine; its progress lines to `log`.
pub async fn provision_lab(dirs: &AppDirs, id: &str, runtime: Runtime, machine: Option<&str>, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    provision(dirs, LabRef { dir: &dir, id, runtime }, machine, log).await
}

/// How lab `id` runs on this machine, for a Stop of what the Machine page found of it there
/// (`in_vm`: its VMs, else its containers). `None` when it isn't an installed lab that runs here
/// (removed since, the self-test, or recorded as running on a server host): its leftovers then go
/// by name.
pub async fn runtime_here(dirs: &AppDirs, id: &str, in_vm: bool) -> Option<Runtime> {
    lab_dir(dirs, id).ok()?;
    let recorded = Recorded::of(dirs, id).await.ok()?;
    if recorded.host().is_some() {
        return None;
    }
    // Its VMs are its machines: a VM lab, or a container lab run as one VM per machine.
    Some(if recorded.per_machine() || in_vm { Runtime::Vm } else { Runtime::Docker })
}

/// Stops lab `id` wherever it runs: the one Stop of the lab page, the Machine page and the
/// hosted agent (through `labs::stop_lab_retiring_colocation`). Marked as stopping from the
/// start: `end_first` (what the layers above end with the lab: its deploy workers, a deploy
/// still running in one of which can take minutes while Terraform is interrupted and waited for,
/// and the website's co-location check, which has nothing left to vouch for) runs first, while
/// the page and the quit guard see a stop and no attack box may be added, so the teardown never
/// races a `vagrant up` or `compose up` that would recreate what it removes; then the lab is torn
/// down.
pub async fn stop_lab(dirs: &AppDirs, id: &str, runtime: Runtime, end_first: impl Future<Output = ()>, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    end_first.await;
    stop(dirs, LabRef { dir: &dir, id, runtime }, log).await
}

#[cfg(test)]
mod tests;
