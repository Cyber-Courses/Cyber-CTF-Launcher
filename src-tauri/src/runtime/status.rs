//! What a lab looks like right now, wherever it runs: its machines and networks, where it runs,
//! whether it is parked, and (for a container lab on a Terraform host) the containers read from
//! that host.

use std::path::Path;

use crate::platform::AppDirs;

use super::host::{self, Host};
use super::paths::lab_dir;
use super::recorded::Recorded;
use super::{LabStatus, Login, Place, Runtime, attack_box, isothrum, server};
use crate::error::Result;

/// The status of lab `id`, installed in `dir` and declared `runtime`, wherever it runs and in the
/// form it was started in.
pub(crate) async fn status(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<LabStatus> {
    // Where it runs, in the form it was started in (a container lab run as one VM per machine
    // reads as a VM lab): the runner's view. A cloud-services lab on no account runs nowhere.
    let host = host::locate(dirs, dir, id, runtime).await?;
    let status = match &host {
        Some(host) => status_at(host).await?,
        None => LabStatus::default(),
    };
    let here = matches!(host, Some(Host::Local(_)));
    // What the page may do on it, by the lifecycle's own rules.
    let controls = super::lifecycle::controls(host.as_ref(), status.provider.as_deref());
    // The attack box the launcher added is no machine of the lab's own (`attack_box`).
    let status = attack_box::set_apart(status, dir)?;
    // The sign-in each service ships with, as the lab's spec declares it beside the port.
    let status = with_logins(status, dir);
    // Here, parked = the run is recorded parked and it is still down.
    let recorded = Recorded::of(dirs, id).await?;
    let status = if here { with_parked(status, recorded.parked()) } else { status };
    // Whether this container lab runs as one VM per machine (its VM recipes), locally or as
    // cloud-vm: the UI reads it to name the form.
    let per_machine = runtime == Runtime::Docker && recorded.per_machine();
    // A deployment the auto-stop couldn't destroy, wherever it is: said on the lab's page.
    let reap_error = super::reaper::last_failure(dirs, id);
    Ok(LabStatus { per_machine, reap_error, controls, ..status })
}

/// `s` with each port's login, read from the lab's spec in `dir` (`login` on a port): matched by
/// machine and TCP port. A spec that doesn't read leaves the ports as they are, without logins.
fn with_logins(mut s: LabStatus, dir: &Path) -> LabStatus {
    if let Ok(spec) = super::lab::load(dir) {
        fill_logins(&mut s, &spec);
    }
    s
}

/// Gives each port of `s` the login `spec` declares on that machine's TCP port.
fn fill_logins(s: &mut LabStatus, spec: &super::lab::Spec) {
    for machine in &mut s.machines {
        let Some(declared) = spec.machines.get(&machine.name) else { continue };
        for port in &mut machine.ports {
            let login = declared.ports.iter().find(|p| p.port == port.target.get() && p.protocol.is_tcp()).and_then(|p| p.login.as_ref());
            port.login = login.map(|l| Login { user: l.user.clone(), password: l.password.clone() });
        }
    }
}

/// Parked = its run is recorded parked (`parked`) and it is still down; a lab someone started
/// again by hand (or that is up for any reason) is simply running.
fn with_parked(s: LabStatus, parked: Option<super::Park>) -> LabStatus {
    LabStatus { parked: parked.filter(|_| !s.running), ..s }
}

/// The lab as the runner sees it where it runs. The failed-launch masking is applied by `status`,
/// uniformly, so it is not repeated here.
async fn status_at(host: &Host) -> Result<LabStatus> {
    Ok(match host {
        // On this machine: a container lab on Docker or in one local VM, or a VM lab.
        Host::Local(run) => run.status().await?,
        // ESXi (Vagrant on the host): Vagrant's states, each VM at its own address.
        Host::Esxi { run, conn } => on_server(run.status().await?, conn),
        // A cloud account (cloud-vm, cloud-docker, cloud-services): from the runner's record.
        Host::Cloud { run, conn } => on_server(run.status().await?, conn),
        // A Proxmox server: from the runner's own record and state, each VM as the server's API
        // says it stands, asked with the server's credentials (a VM removed in its web UI gone).
        Host::Server { run, conn } => on_server(run.status().await?, conn),
    })
}

/// A lab's status as running on the server host or cloud account of connection `conn`: its
/// name, its id (the saved host's, which the page finds it by) and its provider.
fn on_server(status: LabStatus, conn: &server::Connection) -> LabStatus {
    let place = if conn.provider.is_cloud() { Place::Cloud } else { Place::Server };
    LabStatus { host: Some(conn.name.clone()), host_id: Some(conn.id.clone()), place: Some(place), provider: Some(conn.provider.id().to_string()), ..status }
}

/// Something of a lab still up somewhere: its run, in any state but gone (running, starting, a
/// failed start's machines left over, parked), or a deployment of the launcher's own Terraform
/// path not handed to the runner yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Up {
    /// The saved server or cloud account it is on, by id; `None`: this machine.
    pub(crate) host: Option<String>,
    /// That host's name, as the player reads it (its id when it is no longer saved).
    pub(crate) host_name: Option<String>,
    /// Only parked: nothing of it runs, its machines are kept.
    pub(crate) parked: bool,
}

/// Whether something of lab `id` is still up, and where: its run as the runner records it (run
/// `cyberctf-<id>`, in the state its last operation left it in, on the place it was started on:
/// this machine, a server or a cloud account, whatever the form), or a deployment of the
/// launcher's own Terraform path still waiting for its hand-over to the runner
/// (`isothrum::import`). The one rule an update, a re-import, a start somewhere else, a failed
/// website launch and a host's removal go by.
///
/// The runner keeps its record of a run (and Vagrant's) under the app data folder's `runs/`,
/// never in the lab folder, so the lab folder says nothing of it. The record is read from files
/// only: no tool runs and no host is reached. A record that can't be read is an error, never
/// "nothing up".
pub(crate) async fn still_up(dirs: &AppDirs, id: &str) -> Result<Option<Up>> {
    // Named as the player reads it: by its id when it is no longer saved.
    let up = |host: Option<String>, parked: bool| -> Result<Up> {
        let host_name = match &host {
            Some(h) => Some(server::host_name(dirs, h)?.unwrap_or_else(|| h.clone())),
            None => None,
        };
        Ok(Up { host, host_name, parked })
    };
    let run = isothrum::record_of(dirs, id).await?;
    run.filter(|r| r.state != isothrum_runner::RunState::Absent).map(|r| up(r.host_label, r.state == isothrum_runner::RunState::Parked)).transpose()
}

/// Refuses to replace lab `id`'s folder (`dir`), with a new version or a re-import, while
/// something of it is up ([`still_up`]): the run's generated files and the files it bind-mounts or
/// provisions from are in that folder. A copy parked by a park is no reason: it is taken down
/// before the swap (`clear_parked`).
pub(crate) async fn refuse_replace(dirs: &AppDirs, dir: &Path, id: &str) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    match still_up(dirs, id).await? {
        Some(up) if up.parked && Recorded::of(dirs, id).await?.parked().is_some() => Ok(()),
        Some(up) => Err(still_running(&up)),
        None => Ok(()),
    }
}

/// Lab `id`'s run, recorded up on this machine, of which nothing is left where it ran (taken down
/// outside the app: `docker compose down` by hand, a clean-up tool, a VM deleted in its
/// hypervisor's app): taken down through the runner, which clears its record, so an update, a
/// re-import, a removal or a start elsewhere goes ahead instead of being refused as still running
/// while the lab's page says it isn't (#276). Nothing for a run recorded on a server or a cloud
/// (read from files only, as [`still_up`] is), a parked one, one with anything left, or one that
/// can't be read now (Docker stopped): the refusal stands. The caller holds the lab lock.
pub(crate) async fn forget_gone(dirs: &AppDirs, id: &str, log: &mut impl FnMut(String)) -> Result<()> {
    forget_gone_with(dirs, id, |dir| async move { isothrum::recorded_here(dirs, &dir, id).await }, log).await
}

/// [`forget_gone`], the run opened by `open` from the lab's folder ([`isothrum::recorded_here`]:
/// none for a run that isn't this machine's to read).
pub(crate) async fn forget_gone_with<F: Future<Output = Result<Option<isothrum::LabRun>>>>(
    dirs: &AppDirs,
    id: &str,
    open: impl FnOnce(std::path::PathBuf) -> F,
    log: &mut impl FnMut(String),
) -> Result<()> {
    let Some(up) = still_up(dirs, id).await? else { return Ok(()) };
    if up.host.is_some() || up.parked {
        return Ok(());
    }
    let Ok(dir) = lab_dir(dirs, id) else { return Ok(()) };
    let Some(run) = open(dir).await? else { return Ok(()) };
    match run.is_gone().await {
        Ok(true) => {
            log("Nothing of this lab is left where it ran (removed outside Cyber CTF): clearing its record.".into());
            run.down(log).await
        }
        Ok(false) => Ok(()),
        Err(e) => {
            log::warn!("couldn't tell whether {id} is gone ({e}): taken as up");
            Ok(())
        }
    }
}

/// The refusal of an operation that needs the lab down while it is `up`.
pub(crate) fn still_running(up: &Up) -> crate::error::Error {
    use crate::error::{Code, Error};
    Error::coded(match &up.host_name {
        Some(host) => Code::LabRunningOn { host: host.clone() },
        None => Code::LabRunningHere,
    })
}

/// Whether lab `id` is up somewhere before a start: up where its run is, parked included
/// ([`still_up`]). What a failed start finds already up was not brought up by it, and must not
/// be torn down with what it did bring up. When the runner's record can't be read, it counts as
/// up: nothing is torn down on a guess. False when it isn't installed.
pub async fn lab_present(dirs: &AppDirs, id: &str) -> bool {
    if lab_dir(dirs, id).is_err() {
        return false;
    }
    match still_up(dirs, id).await {
        Ok(up) => up.is_some(),
        Err(e) => {
            log::warn!("couldn't tell whether {id} is up ({e}); taken for up");
            true
        }
    }
}

/// Whether lab `id` is installed here.
pub fn lab_installed(dirs: &AppDirs, id: &str) -> bool {
    lab_dir(dirs, id).is_ok()
}

/// Where a running container lab is reachable on this machine, for the website to open after a
/// launch: as the runner's status publishes the lab's ports ([`isothrum::LabRun::local_url`]).
/// None for a lab run as VMs or elsewhere, or when nothing is published.
pub async fn primary_url(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Option<String> {
    match host::locate(dirs, dir, id, runtime).await.ok()?? {
        Host::Local(run) if run.target() == Some(super::lab::Target::Docker) => run.local_url().await.ok().flatten(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
