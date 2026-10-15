//! Starting a lab: on this machine (local Docker, a container lab in one local VM, or one VM per
//! machine), or on a server host (ESXi through Vagrant, Proxmox and the clouds through Terraform).

use std::path::Path;

use crate::AppHandle;
use tauri::Manager;

use super::inflight::{Action, DeployGuard};
use super::lifecycle::resume_locked;
use super::paths::{
    deployment_states, forget_lab_host_keys, local_vm, mark_local_vm, mark_parked, parked, record_host, record_runtime, runner_home, state_dir, vagrant_dirs,
};
use super::providers::Provider;
use super::{Runtime, attack_box, attack_vm, cloud_services, docker, exegol, isothrum, lab, server, ssh, terraform, vm};
use crate::error::{Error, Result};

mod local;
mod on_host;

use local::{check_qemu, ensure_local_slot_free, start_local_vm, warn_if_low_memory};
#[cfg(test)]
use local::{emulated_arches, low_memory_warning};
use on_host::start_on_server;
#[cfg(test)]
use on_host::{agreed_ip, budget_verdict, is_public_ipv4, parse_ipv4, public_ip_from};
pub(super) use on_host::{check_aws_budget, public_ip};

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
    if runtime == Runtime::Docker && provider.is_none_or(Provider::is_remote) && isothrum::chosen() {
        return start_with_runner(app, dir, id, env, log).await;
    }
    if resumes_parked(dir, id, runtime, log).await? {
        log("This lab is parked on this machine: resuming it as it was. Stop it first if you want a fresh copy.".into());
        return resume_locked(app, dir, id, runtime, log).await;
    }
    // A lab runs in one place at a time: if it's already up here, refuse (a second copy
    // collides on its published host ports); if an earlier start left stopped or partial
    // infrastructure behind, clear it so this start is clean.
    ensure_local_slot_free(dir, id, log).await?;
    let run = local_run(runtime, provider)?;
    // What it downloads (its images, or its boxes) is recorded as the app's, for Storage to
    // clean up: only what wasn't on this machine before.
    let before = crate::machine::storage::owned::Before::take(run == LocalRun::Docker, run != LocalRun::Docker).await;
    let started = match run {
        LocalRun::DockerInVm(provider) => start_docker_in_vm(dir, runtime, provider, env, log).await,
        LocalRun::Docker => start_on_docker(dir, id, env, log).await,
        LocalRun::Vm(provider) => start_vm_lab(dir, runtime, provider, env, log).await,
    };
    crate::machine::storage::claim_lab(app, dir, id, &before).await;
    started
}

/// Whether this start resumes the lab parked here rather than building it. A parked lab (paused
/// or shut down) is one the player wants back as it was, not rebuilt: its stopped machines would
/// otherwise read as leftovers and be cleared. Unless nothing of it is left to resume: a parked
/// container lab whose stopped containers were pruned (or Docker reset) can only start fresh,
/// and resuming would fail every time.
async fn resumes_parked(dir: &Path, id: &str, runtime: Runtime, log: &mut impl FnMut(String)) -> Result<bool> {
    if parked(dir).is_some() && runtime == Runtime::Docker && local_vm(dir).is_none() && docker::containers(dir, id).await.is_ok_and(|n| n == 0) {
        log("This lab was shut down here, but its stopped containers are gone: starting it fresh.".into());
        mark_parked(dir, None)?;
    }
    // Same for VMs deleted at the hypervisor (or Vagrant's record of them lost): resuming would
    // build blank, unprovisioned machines that then read as the lab.
    if parked(dir).is_some() && (runtime == Runtime::Vm || local_vm(dir).is_some()) && !vm::has_machines(&lab::vagrant_dir(dir, runtime)) {
        log("This lab was shut down here, but its virtual machines are gone: starting it fresh.".into());
        mark_parked(dir, None)?;
    }
    Ok(parked(dir).is_some())
}

/// A container lab in a VM on this machine: Isoloom's docker-vm (Docker on one VM), with the
/// attack box next to it inside the VM (the lab network isn't reachable from here).
async fn start_docker_in_vm(dir: &Path, runtime: Runtime, provider: Provider, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    let prepared = lab::prepare(dir, lab::vagrant_target(runtime))?;
    mark_local_vm(dir, Some(provider))?;
    log(format!("Running in a {} VM on this machine", provider.id()));
    let vagrant = lab::vagrant_dir(dir, runtime);
    start_local_vm(&vagrant, provider, env, log).await?;
    attack_box::in_lab_vm_or_note(&vagrant, &prepared.spec, env, log).await;
    finish_vagrant(dir, &prepared, runtime, log);
    Ok(())
}

/// A VM lab on this machine, one VM per machine on `provider`, with the learner's attack VM.
async fn start_vm_lab(dir: &Path, runtime: Runtime, provider: Provider, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    let prepared = lab::prepare(dir, lab::vagrant_target(runtime))?;
    warn_if_low_memory(&prepared.spec, log);
    if provider == Provider::Qemu {
        check_qemu(&prepared, log).await?;
    }
    let vagrant = lab::vagrant_dir(dir, runtime);
    start_local_vm(&vagrant, provider, env, log).await?;
    finish_vagrant(dir, &prepared, runtime, log);
    // The lab's networks are internal to the hypervisor, so the learner's attack VM (their own
    // box, chosen in Settings) goes beside it, on the same hypervisor. The lab is up at this
    // point: a failing attacker is noted, not a failed deploy.
    if let Some(b) = attack_box::attack_vm_box(env)
        && let Err(e) = attack_vm::start(dir, &vagrant, provider, b, &mut *log).await
    {
        log(format!("The lab is running, but its attack VM didn't start: {e}. Start it from the lab page to retry."));
    }
    Ok(())
}

/// A container lab on this machine's Docker: generate its Compose project, bring it up, plug a
/// running attack box back into its networks, and say where it answers. Takes the lab folder
/// (no app handle), so the real-lab tests run exactly this.
pub(super) async fn start_on_docker(dir: &Path, id: &str, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    recheck_instance(dir, id, log).await?;
    let prepared = lab::prepare(dir, lab::Target::Docker)?;
    mark_local_vm(dir, None)?;
    docker::start(dir, id, env, &mut *log).await?;
    exegol::rejoin(id).await;
    // Its ports here are the ones picked at its first start, not the spec's.
    let published = docker::published(dir, id, env).await;
    if let Some(m) = lab::message_at(dir, &prepared.lab, lab::Target::Docker, &named_ports(&prepared.spec, &published)) {
        log_lines(&m, log);
    }
    Ok(())
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
    isothrum::DockerRun::new(home, dir, id)?.up(env, log).await?;
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

/// The end of a Vagrant start (local or ESXi): the lab's welcome message.
fn finish_vagrant(dir: &Path, prepared: &lab::Prepared, runtime: Runtime, log: &mut impl FnMut(String)) {
    welcome(dir, &prepared.lab, lab::vagrant_target(runtime), log);
}

/// The host ports Compose published, as (machine, port name, host port): each (service, port,
/// protocol, host port) under the name the lab gives that machine's port.
fn named_ports(spec: &lab::Spec, published: &[(String, u16, String, u16)]) -> Vec<(String, String, u16)> {
    published
        .iter()
        .filter_map(|(machine, port, protocol, host)| {
            let declared = spec.machines.get(machine)?.ports.iter().find(|p| p.port == *port && p.protocol.id() == protocol)?;
            Some((machine.clone(), declared.name.clone().unwrap_or_else(|| port.to_string()), *host))
        })
        .collect()
}

/// The lab's own words once it is up (`message:` in its spec, addresses filled in): where to
/// start and what to do first, as the last lines of the deploy log.
fn welcome(dir: &Path, lab: &lab::Lab, target: lab::Target, log: &mut impl FnMut(String)) {
    if let Some(m) = lab::message(dir, lab, target) {
        log_lines(&m, log);
    }
}

fn log_lines(text: &str, log: &mut impl FnMut(String)) {
    text.lines().for_each(|l| log(l.to_string()));
}

#[cfg(test)]
mod tests;
