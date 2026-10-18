//! Starting a lab: on this machine (local Docker, a container lab in one local VM, or one VM per
//! machine), or on a server host (ESXi through Vagrant, Proxmox and the clouds through Terraform).

use std::path::Path;

use crate::AppHandle;
use tauri::Manager;

use super::inflight::{Action, DeployGuard};
use super::paths::{deployment_states, forget_lab_host_keys, mark_local_vm, mark_parked, parked, runner_home, vagrant_dirs};
use super::providers::Provider;
use super::{Runtime, cloud_services, docker, isothrum, lab, server, terraform, vm};
use crate::error::{Error, Result};

mod local;
mod on_host;

use local::{check_qemu, warn_if_low_memory};
#[cfg(test)]
use local::{emulated_arches, low_memory_warning};
use on_host::start_on_server;
#[cfg(test)]
use on_host::{agreed_ip, budget_verdict, is_public_ipv4, parse_ipv4, public_ip_from};
pub(super) use on_host::{check_aws_budget, public_ip};
pub(super) use on_host::{esxi_run, runner_runs_on, server_place};

/// Starts an installed lab; `env` is passed to the runtime (evidence claim, attack box).
///
/// - No `host`: Docker labs run on this machine's Docker, or in one VM on this machine with a
///   local `provider` (Isoloom's docker-vm); VM labs run one VM per machine (Isoloom's vagrant).
/// - With a server `host`: the lab runs there. ESXi runs the same Vagrantfiles; Proxmox and
///   the clouds run Isoloom's Terraform (Docker on one VM, or one VM per machine on Proxmox).
/// - Cloud-services labs (`Runtime::Cloud`) run only on a cloud account `host`.
///
/// The caller holds the lab lock (`lock_lab`): a launch holds it from the download on.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_locked(
    app: &AppHandle,
    dir: &Path,
    id: &str,
    runtime: Runtime,
    provider: Option<Provider>,
    host: Option<&str>,
    env: &[(String, String)],
    mut log: impl FnMut(String),
) -> Result<()> {
    let _deploy = DeployGuard::new(id, Action::Start);
    // A lab runs in one place: starting it here (or on another host) while its deployment is
    // still up elsewhere would drop the marker naming that host, and with it the way to stop
    // it (a cloud lab would keep billing).
    if let Ok(base) = app.path().app_data_dir()
        && let Some(other) = deployed_elsewhere(&base, dir, id, host)
    {
        let name = server::host_name(app, &other).unwrap_or(other);
        return Err(Error::Refused(format!("This lab is still running on {name}. Stop it there before starting it somewhere else.")));
    }
    // New hosts may come up at the addresses of others: no key pinned before counts. A parked
    // lab resumes its own machines, whose keys stay pinned.
    if parked(dir).is_none() {
        forget_lab_host_keys(dir);
    }
    if runtime == Runtime::Cloud {
        return cloud_services::start(app, dir, id, host, env, &mut log).await;
    }
    match host {
        None => start_here(app, dir, id, runtime, provider, env, &mut log).await,
        Some(host) => start_on_server(app, dir, id, runtime, host, env, &mut log).await,
    }
}

/// [`start_locked`] under the lab lock, for tests that start an installed lab directly.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn start(
    app: &AppHandle,
    dir: &Path,
    id: &str,
    runtime: Runtime,
    provider: Option<Provider>,
    host: Option<&str>,
    env: &[(String, String)],
    log: impl FnMut(String),
) -> Result<()> {
    let _lock = super::inflight::lock_lab(app, id).await?;
    start_locked(app, dir, id, runtime, provider, host, env, log).await
}

/// The name of the server host lab `id` (folder `dir`) is deployed on, if something of it is
/// still there: its folder must stay as it is (Vagrant's record of its machines, the module its
/// Terraform state was applied from) until it is stopped there.
pub fn deployed_on_server(app: &AppHandle, dir: &Path, id: &str) -> Option<String> {
    let host = deployed_elsewhere(&app.path().app_data_dir().ok()?, dir, id, None)?;
    Some(server::host_name(app, &host).unwrap_or(host))
}

/// The server host lab `id` (folder `dir`, app data folder `base`) is still deployed on, when
/// it is not `host` (`None`: this machine): its marker names that host and something of it is
/// left there (a Terraform state holding resources, or machines Vagrant still tracks).
fn deployed_elsewhere(base: &Path, dir: &Path, id: &str, host: Option<&str>) -> Option<String> {
    let marked = server::marked_host(dir).filter(|h| !h.is_empty())?;
    if host == Some(marked.as_str()) {
        return None;
    }
    let terraform = deployment_states(base, id).iter().any(|(_, state)| terraform::deployed(state));
    let vagrant = vagrant_dirs(dir).any(|v| vm::has_machines(&v));
    (terraform || vagrant).then_some(marked)
}

async fn start_here(
    app: &AppHandle,
    dir: &Path,
    id: &str,
    runtime: Runtime,
    provider: Option<Provider>,
    env: &[(String, String)],
    log: &mut impl FnMut(String),
) -> Result<()> {
    server::mark_lab(dir, None)?;
    // Every lab on this machine runs through the Isothrum runner. A parked run is resumed as it
    // was and leftovers are cleared first (the runner's `Occupied::ResumeParked`); the runner
    // wrappers record the lab's downloads for Storage.
    match local_run(runtime, provider)? {
        LocalRun::Docker => start_with_runner(app, dir, id, env, log).await,
        LocalRun::DockerInVm(p) => start_docker_in_vm_with_runner(app, dir, id, VmStart { provider: p, env }, log).await,
        LocalRun::Vm(p) => start_vm_with_runner(app, dir, id, VmStart { provider: p, env }, log).await,
    }
}

/// A container lab in a VM on this machine through the Isothrum runner, its boxes recorded for
/// Storage as on the launcher's own path.
async fn start_docker_in_vm_with_runner(app: &AppHandle, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let before = crate::machine::storage::owned::Before::take(false, true).await;
    let started = start_docker_in_vm_run(&runner_home(app)?, dir, id, start, log).await;
    crate::machine::storage::claim_lab(app, dir, id, &before).await;
    started
}

/// The runner's start of a container lab in one VM (Isoloom's `docker-vm`, runner home `home`) on
/// the local hypervisor: its files generated, then up, then the attack box inside the VM beside
/// the containers (the lab network isn't reachable from here). Takes no app handle, so the
/// real-lab tests run exactly this.
pub(super) async fn start_docker_in_vm_run(home: &Path, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let VmStart { provider, env } = start;
    lab::prepare(dir, lab::vagrant_target(Runtime::Docker))?;
    mark_local_vm(dir, Some(provider))?;
    log(format!("Running in a {} VM on this machine", provider.id()));
    isothrum::LabRun::docker_in_vm(home, dir, id)?.up(env, Some(provider.id()), log).await?;
    mark_parked(dir, None)
}

/// A container lab on this machine's Docker through the Isothrum runner (see `isothrum`), its
/// downloads recorded for Storage as on the launcher's own path.
async fn start_with_runner(app: &AppHandle, dir: &Path, id: &str, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    let before = crate::machine::storage::owned::Before::take(true, false).await;
    let started = start_docker_run(&runner_home(app)?, dir, id, env, log).await;
    crate::machine::storage::claim_lab(app, dir, id, &before).await;
    started
}

/// The runner's start of a container lab (runner home `home`): its files generated (the run
/// manifest with them), then up, or resumed as it was when it is parked (the runner's own rule).
/// Takes no app handle, so the real-lab tests run exactly this.
pub(super) async fn start_docker_run(home: &Path, dir: &Path, id: &str, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    recheck_instance(dir, id, log).await?;
    lab::prepare(dir, lab::Target::Docker)?;
    mark_local_vm(dir, None)?;
    isothrum::LabRun::docker(home, dir, id)?.up(env, None, log).await?;
    mark_parked(dir, None)
}

/// What a VM lab on this machine starts with: its hypervisor, and the launch's values.
#[derive(Debug, Clone, Copy)]
pub(super) struct VmStart<'a> {
    /// The local hypervisor (Vagrant provider).
    pub(super) provider: Provider,
    /// The launch's values: the inputs the lab's slots take, and the ports choice.
    pub(super) env: &'a [(String, String)],
}

/// A VM lab on this machine through the Isothrum runner, its boxes recorded for Storage as on
/// the launcher's own path.
async fn start_vm_with_runner(app: &AppHandle, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let before = crate::machine::storage::owned::Before::take(false, true).await;
    let started = start_vm_run(&runner_home(app)?, dir, id, start, log).await;
    crate::machine::storage::claim_lab(app, dir, id, &before).await;
    started
}

/// The runner's start of a VM lab (runner home `home`) on `provider`: its files generated (the
/// run manifest with them), the memory warning and QEMU's emulator check, then up (a parked run
/// is resumed as it was). The learner's attack box is hot-added into the run on demand, from the
/// lab page, not at launch. Takes no app handle, so the real-lab tests run exactly this.
pub(super) async fn start_vm_run(home: &Path, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let VmStart { provider, env } = start;
    let prepared = lab::prepare(dir, lab::Target::Vagrant)?;
    warn_if_low_memory(&prepared.spec, log);
    if provider == Provider::Qemu {
        check_qemu(&prepared, log).await?;
    }
    isothrum::LabRun::vm(home, dir, id)?.up(env, Some(provider.id()), log).await?;
    mark_parked(dir, None)
}

/// Checks the lab's instance against the networks Docker has now (an install with Docker down
/// picked it blind), moving the lab (stopped: its slot was cleared) to a free one if its blocks
/// are taken. With Docker down there is nothing to check against, and the start says so next.
async fn recheck_instance(dir: &Path, id: &str, log: &mut impl FnMut(String)) -> Result<()> {
    let Some(labs) = dir.parent() else { return Ok(()) };
    let before = lab::instance(dir);
    let now = lab::recheck_instance(labs, dir, &docker::subnets_in_use(id).await)?;
    if before.is_some_and(|b| b != now) {
        log(format!("Another Docker network took this lab's addresses: it moves to free ones (instance {now})."));
    }
    Ok(())
}

/// Cloud-services labs have nothing to run here.
pub(super) const CLOUD_NEEDS_ACCOUNT: &str = "This lab runs in your cloud account: pick one of your cloud accounts to start it on.";

/// How a lab runs on this machine.
#[derive(Debug, PartialEq)]
enum LocalRun {
    /// A container lab in one VM on this hypervisor (Isoloom's docker-vm).
    DockerInVm(Provider),
    /// A container lab on this machine's Docker.
    Docker,
    /// A VM lab, one VM per machine on this hypervisor.
    Vm(Provider),
}

/// The local run for `runtime` on `provider`: a container lab with a local hypervisor runs in a
/// VM there, without one on Docker; a VM lab needs a local hypervisor.
fn local_run(runtime: Runtime, provider: Option<Provider>) -> Result<LocalRun> {
    match runtime {
        Runtime::Docker => Ok(match provider.filter(|p| !p.is_remote()) {
            Some(p) => LocalRun::DockerInVm(p),
            None => LocalRun::Docker,
        }),
        Runtime::Vm => {
            let provider = provider.ok_or_else(|| Error::Invalid("VM labs need a provider".into()))?;
            if provider.is_remote() {
                return Err(Error::Invalid("pick a server host to run on ESXi or Proxmox".into()));
            }
            Ok(LocalRun::Vm(provider))
        }
        Runtime::Cloud => Err(Error::Invalid(CLOUD_NEEDS_ACCOUNT.into())),
    }
}

#[cfg(test)]
mod tests;
