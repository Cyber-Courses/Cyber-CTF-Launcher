//! Starting a lab: on this machine (local Docker, a container lab in one local VM, or one VM per
//! machine), or on a server host (ESXi through Vagrant, Proxmox and the clouds through Terraform).

use std::path::Path;

use crate::platform::AppDirs;

use super::inflight::{Action, DeployGuard};
use super::paths::{deployment_states, forget_lab_host_keys, mark_engine, mark_local_vm, mark_parked, mark_per_machine_vm, parked, runner_home, vagrant_dirs};
use super::providers::Provider;
use super::{LabRef, Runtime, cloud_services, docker, isothrum, lab, server, terraform, vm};
use crate::error::{Error, Result};

mod local;
mod on_host;
mod port_holder;

use local::{check_provider, check_qemu, check_utm, warn_if_low_memory};
#[cfg(test)]
use local::{emulated_arches, low_memory_warning};
pub(in crate::runtime) use on_host::esxi_run;
use on_host::start_on_server;
#[cfg(test)]
pub(crate) use on_host::{CloudStart, start_cloud_run};
#[cfg(test)]
use on_host::{agreed_ip, budget_verdict, is_public_ipv4, parse_ipv4, per_machine_runs_on, public_ip_from, record_cloud_form};
pub(super) use on_host::{check_aws_budget, public_ip};
pub(super) use on_host::{runner_runs_on, server_place};

/// Where a lab is started: on this machine (with a local `provider`, and `per_machine_vm`
/// picking the one-VM-per-machine form for a container lab), or on the server `host`.
#[derive(Clone, Copy, Default)]
pub(crate) struct Destination<'a> {
    pub(crate) provider: Option<Provider>,
    pub(crate) host: Option<&'a str>,
    pub(crate) per_machine_vm: bool,
    /// The Docker context a container lab here runs on (the learner's engine); none: the
    /// current one.
    pub(crate) docker_context: Option<&'a str>,
}

/// Starts an installed lab; `env` is passed to the runtime (evidence claim, attack box).
///
/// - No `host`: Docker labs run on this machine's Docker, or in one VM on this machine with a
///   local `provider` (Isoloom's docker-vm), or, with `per_machine_vm`, as one VM per machine
///   like a VM lab when their spec has VM recipes too; VM labs run one VM per machine
///   (Isoloom's vagrant).
/// - With a server `host`: the lab runs there. ESXi runs the same Vagrantfiles; Proxmox and
///   the clouds run Isoloom's Terraform (Docker on one VM, or one VM per machine on Proxmox).
/// - Cloud-services labs (`Runtime::Cloud`) run only on a cloud account `host`.
///
/// The caller holds the lab lock (`lock_lab`): a launch holds it from the download on.
pub(crate) async fn start_locked(dirs: &AppDirs, lab: LabRef<'_>, to: Destination<'_>, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    let Destination { provider, host, per_machine_vm, docker_context } = to;
    let _deploy = DeployGuard::new(id, Action::Start);
    // A lab runs in one place: starting it here (or on another host) while its deployment is
    // still up elsewhere would drop the marker naming that host, and with it the way to stop
    // it (a cloud lab would keep billing).
    if let Ok(base) = dirs.data()
        && let Some(other) = deployed_elsewhere(base, dir, id, host)
    {
        let name = server::host_name(dirs, &other).unwrap_or(other);
        return Err(Error::Refused(format!("This lab is still running on {name}. Stop it there before starting it somewhere else.")));
    }
    // New hosts may come up at the addresses of others: no key pinned before counts. A parked
    // lab resumes its own machines, whose keys stay pinned.
    if parked(dir).is_none() {
        forget_lab_host_keys(dir);
    }
    if runtime == Runtime::Cloud {
        return cloud_services::start(dirs, lab, host, env, &mut log).await;
    }
    match host {
        None => start_here(dirs, lab, local_run(runtime, provider, per_machine_vm)?, docker_context, env, &mut log).await,
        // On a cloud account the per-machine form is cloud-vm (one instance per machine); on a
        // server (ESXi, Proxmox) the lab runs as the catalogue says, so `start_on_server` refuses
        // the per-machine pick there. The dispatch on the provider is its to make (the connection).
        Some(host) => start_on_server(dirs, lab, (host, per_machine_vm), env, &mut log).await,
    }
}

/// [`start_locked`] under the lab lock, for tests that start an installed lab directly.
#[cfg(test)]
pub(crate) async fn start(dirs: &AppDirs, lab: LabRef<'_>, to: Destination<'_>, env: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    let _lock = super::inflight::lock_lab(dirs, lab.id).await?;
    start_locked(dirs, lab, to, env, log).await
}

/// The name of the server host lab `id` (folder `dir`) is deployed on, if something of it is
/// still there: its folder must stay as it is (Vagrant's record of its machines, the module its
/// Terraform state was applied from) until it is stopped there.
pub fn deployed_on_server(dirs: &AppDirs, dir: &Path, id: &str) -> Option<String> {
    let host = deployed_elsewhere(dirs.data().ok()?, dir, id, None)?;
    Some(server::host_name(dirs, &host).unwrap_or(host))
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

/// Starts the lab on this machine as `run` says (see [`local_run`]).
#[allow(
    clippy::too_many_arguments,
    reason = "the local-launch helper mirrors start_locked's launch specification: the lab, how it runs, its engine, its inputs and a log sink"
)]
async fn start_here(
    dirs: &AppDirs,
    lab: LabRef<'_>,
    run: LocalRun,
    docker_context: Option<&str>,
    env: &[(String, String)],
    log: &mut impl FnMut(String),
) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    server::mark_lab(dir, None)?;
    // The form is remembered with the lab before anything runs: every later operation on it
    // (status, stop, park) follows the form it was started in, whatever the catalogue calls it.
    // A container lab started as one VM per machine is the one case where the two differ.
    mark_per_machine_vm(dir, runtime == Runtime::Docker && matches!(run, LocalRun::Vm(_)))?;
    // The container engine the player chose for a plain container lab (a Docker context other than
    // the current one), so every later docker/compose call for it — launcher and runner — goes to
    // that engine. Only containers on Docker here (not a VM form); nothing recorded = current context.
    mark_engine(dir, if matches!(run, LocalRun::Docker) { docker_context } else { None })?;
    // Every lab on this machine runs through the Isothrum runner. A parked run is resumed as it
    // was and leftovers are cleared first (the runner's `Occupied::ResumeParked`); the runner
    // wrappers record the lab's downloads for Storage.
    let started = match run {
        LocalRun::Docker => start_with_runner(dirs, dir, id, env, log).await,
        LocalRun::DockerInVm(p) => start_docker_in_vm_with_runner(dirs, dir, id, VmStart { provider: p, env }, log).await,
        LocalRun::Vm(p) => start_vm_with_runner(dirs, dir, id, VmStart { provider: p, env }, log).await,
    };
    // A clash is on a host port either form binds here, so name what holds it: the player can
    // then stop it and start again, rather than guess what "port 8080 is taken" means.
    match started {
        Err(e) => Err(port_holder::with_holder(e).await),
        ok => ok,
    }
}

/// A container lab in a VM on this machine through the Isothrum runner, its boxes recorded for
/// Storage as on the launcher's own path.
async fn start_docker_in_vm_with_runner(dirs: &AppDirs, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let before = crate::machine::storage::owned::Before::take(false, true).await;
    let started = start_docker_in_vm_run(&runner_home(dirs)?, dir, id, start, log).await;
    crate::machine::storage::claim_lab(dirs.clone(), dir, id, &before).await;
    started
}

/// The runner's start of a container lab in one VM (Isoloom's `docker-vm`, runner home `home`) on
/// the local hypervisor: its files generated, then up. Takes no app handle, so the integration
/// tests run exactly this.
pub(crate) async fn start_docker_in_vm_run(home: &Path, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let VmStart { provider, env } = start;
    lab::prepare(dir, lab::vagrant_target(Runtime::Docker))?;
    mark_local_vm(dir, Some(provider))?;
    log(format!("Running in a {} VM on this machine", provider.id()));
    if provider == Provider::Utm {
        check_utm(log).await?;
    }
    let run = isothrum::LabRun::docker_in_vm(home, dir, id)?;
    let mut tried_again = false;
    loop {
        // Marked before every attempt: the teardown of a failed one forgets the marker with the
        // rest of the run, and without it status, stop and the attack box would read the lab
        // as plain Docker on this machine once the next attempt succeeds.
        mark_local_vm(dir, Some(provider))?;
        let Err(e) = run.up(env, Some(provider.id()), log).await else {
            return mark_parked(dir, None);
        };
        // A failed start can leave a half-created helper VM holding host ports (e.g. the QEMU
        // SSH forward), and that lingering VM trips the next launch with a port-collision
        // error. Destroy it before surfacing the failure; the teardown is best-effort, so its
        // own error is logged, never allowed to mask the start's.
        log("Start failed: removing the half-created VM so the next launch starts clean".into());
        let teardown = match isothrum::LabRun::docker_in_vm(home, dir, id) {
            Ok(vm) => super::lifecycle::stop_run(vm, dir, &mut *log).await,
            Err(e) => Err(e),
        };
        if let Err(teardown) = teardown {
            log(format!("Could not fully remove the failed VM: {teardown}"));
        }
        if provider == Provider::Utm && !tried_again && isothrum::errors::try_again_after_utm(&e, log) {
            tried_again = true;
            continue;
        }
        return Err(e);
    }
}

/// A container lab on this machine's Docker through the Isothrum runner (see `isothrum`), its
/// downloads recorded for Storage as on the launcher's own path.
async fn start_with_runner(dirs: &AppDirs, dir: &Path, id: &str, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    let before = crate::machine::storage::owned::Before::take(true, false).await;
    let started = start_docker_run(&runner_home(dirs)?, dir, id, env, log).await;
    crate::machine::storage::claim_lab(dirs.clone(), dir, id, &before).await;
    started
}

/// The runner's start of a container lab (runner home `home`): its files generated (the run
/// manifest with them), then up, or resumed as it was when it is parked (the runner's own rule).
/// Takes no app handle, so the integration tests run exactly this.
pub(crate) async fn start_docker_run(home: &Path, dir: &Path, id: &str, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    recheck_instance(dir, id, log).await?;
    lab::prepare(dir, lab::Target::Docker)?;
    mark_local_vm(dir, None)?;
    isothrum::LabRun::docker(home, dir, id)?.up(env, None, log).await?;
    mark_parked(dir, None)
}

/// What a VM lab on this machine starts with: its hypervisor, and the launch's values.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VmStart<'a> {
    /// The local hypervisor (Vagrant provider).
    pub(crate) provider: Provider,
    /// The launch's values: the inputs the lab's slots take, and the ports choice.
    pub(crate) env: &'a [(String, String)],
}

/// A VM lab on this machine through the Isothrum runner, its boxes recorded for Storage as on
/// the launcher's own path.
async fn start_vm_with_runner(dirs: &AppDirs, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let before = crate::machine::storage::owned::Before::take(false, true).await;
    let started = start_vm_run(&runner_home(dirs)?, dir, id, start, log).await;
    crate::machine::storage::claim_lab(dirs.clone(), dir, id, &before).await;
    started
}

/// The runner's start of a VM lab (runner home `home`) on `provider`: its files generated (the
/// run manifest with them), the memory warning and QEMU's emulator check, then up (a parked run
/// is resumed as it was). The learner's attack box is added to the lab afterwards, from the lab
/// page or with the lab (`attack_machine`), not by the launch. Takes no app handle, so the
/// integration tests run exactly this.
pub(crate) async fn start_vm_run(home: &Path, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let VmStart { provider, env } = start;
    let prepared = lab::prepare(dir, lab::Target::Vagrant)?;
    // A container lab's VM form leaves no local-VM mark behind: that one names docker-vm.
    mark_local_vm(dir, None)?;
    check_provider(&prepared, provider)?;
    warn_if_low_memory(&prepared.spec, log);
    if provider == Provider::Qemu {
        check_qemu(&prepared, log).await?;
    }
    if provider == Provider::Utm {
        check_utm(log).await?;
    }
    let run = isothrum::LabRun::vm(home, dir, id)?;
    if let Err(e) = run.up(env, Some(provider.id()), log).await {
        if provider != Provider::Utm || !isothrum::errors::try_again_after_utm(&e, log) {
            return Err(e);
        }
        // A `vagrant up` after UTM's passing refusal resumes the VMs as they were left.
        run.up(env, Some(provider.id()), log).await?;
    }
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

/// The per-machine form is a pick for this machine.
pub(super) const PER_MACHINE_HERE_ONLY: &str = "One VM per machine is a way to run the lab on this machine: on a server, the lab runs as it is published.";

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
/// VM there, without one on Docker, and with `per_machine_vm` as one VM per machine (its spec's
/// VM recipes, as a VM lab); a VM lab needs a local hypervisor.
fn local_run(runtime: Runtime, provider: Option<Provider>, per_machine_vm: bool) -> Result<LocalRun> {
    match runtime {
        Runtime::Docker if per_machine_vm => {
            let provider = provider.filter(|p| !p.is_remote()).ok_or_else(|| Error::Invalid("One VM per machine needs a hypervisor on this machine".into()))?;
            Ok(LocalRun::Vm(provider))
        }
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
