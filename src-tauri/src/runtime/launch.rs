//! Starting a lab: on this machine (local Docker, a container lab in one local VM, or one VM per
//! machine), or on a server host (ESXi through Vagrant, Proxmox and the clouds through Terraform).

use std::path::Path;

use crate::platform::AppDirs;

use super::inflight::{Action, DeployGuard};
pub(crate) use super::isothrum::Inputs;
use super::paths::{mark_engine, mark_local_vm, mark_parked, mark_per_machine_vm, runner_home};
use super::providers::Provider;
use super::{LabRef, Runtime, cloud_services, docker, isothrum, lab, server};
use crate::error::{Code, Error, Result};
use crate::platform::policy::Key;

mod local;
mod on_host;
mod port_holder;

#[cfg(test)]
use local::low_memory_warning;
use local::{check_provider, check_qemu, check_utm, warn_if_low_memory};
pub(in crate::runtime) use on_host::esxi_run;
#[cfg(test)]
pub(crate) use on_host::{CloudStart, start_cloud_run};
#[cfg(test)]
use on_host::{agreed_ip, budget_verdict, is_public_ipv4, parse_ipv4, public_ip_from, record_cloud_form};
pub(super) use on_host::{check_aws_budget, public_ip};
use on_host::{per_machine_runs_on, start_on_server};
pub(super) use on_host::{runner_runs_on, server_place};

/// Where a lab is started: on this machine (with a local `provider`, and `per_machine_vm`
/// picking the one-VM-per-machine form for a container lab), or on the server `host`; and
/// whether its services keep their own host ports there (`default_ports`).
#[derive(Clone, Copy, Default)]
pub(crate) struct Destination<'a> {
    /// The hypervisor it runs on here (a container lab's VM's, in a VM form); None on Docker or on
    /// a server.
    pub(crate) provider: Option<Provider>,
    /// The saved server or cloud account it starts on, by id; None for this machine.
    pub(crate) host: Option<&'a str>,
    /// A container lab started as one VM per machine (its VM recipes) rather than as containers.
    pub(crate) per_machine_vm: bool,
    /// The Docker context a container lab here runs on (the learner's engine); none: the
    /// current one.
    pub(crate) docker_context: Option<&'a str>,
    /// The player chose the lab's own host ports (`CTFd` on 8000) over free ones picked at random.
    pub(crate) default_ports: bool,
}

/// A [`Destination`] that owns its values: what a launch carries from the app to the deploy worker
/// that runs it (in its job file). Each value missing from a job file is its default.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Placement {
    /// [`Destination::provider`].
    pub(crate) provider: Option<Provider>,
    /// [`Destination::host`].
    pub(crate) host: Option<String>,
    /// [`Destination::per_machine_vm`].
    pub(crate) per_machine_vm: bool,
    /// [`Destination::docker_context`].
    pub(crate) docker_context: Option<String>,
    /// [`Destination::default_ports`].
    pub(crate) default_ports: bool,
}

impl Placement {
    /// Where the lab starts, borrowed from this placement.
    pub(crate) fn destination(&self) -> Destination<'_> {
        Destination {
            provider: self.provider,
            host: self.host.as_deref(),
            per_machine_vm: self.per_machine_vm,
            docker_context: self.docker_context.as_deref(),
            default_ports: self.default_ports,
        }
    }
}

/// Starts an installed lab; `env` holds the lab's input values (the evidence claim, the player's
/// flags).
///
/// - No `host`: Docker labs run on this machine's Docker, or in one VM on this machine with a
///   local `provider` (Isoloom's docker-vm), or, with `per_machine_vm`, as one VM per machine
///   like a VM lab when their spec has VM recipes too; VM labs run one VM per machine
///   (Isoloom's vagrant).
/// - With a server `host`: the lab runs there. ESXi runs the same Vagrantfiles; Proxmox and
///   the clouds run Isoloom's Terraform (Docker on one VM, or one VM per machine on Proxmox).
/// - Cloud-services labs (`Runtime::Cloud`) run only on a cloud account `host`.
///
/// The caller holds the lab lock (`lock_lab`): a launch holds it from the download on. A start
/// with nowhere to run, or that the administrator's policy turns off, is refused before anything
/// changes ([`destination`], then [`privileged_allowed`]).
pub(crate) async fn start_locked(dirs: &AppDirs, lab: LabRef<'_>, to: Destination<'_>, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    let place = destination(dirs, runtime, &to)?;
    let Destination { host, per_machine_vm, docker_context, default_ports, .. } = to;
    let inputs = Inputs { values: env, default_ports };
    let _deploy = DeployGuard::new(id, Action::Launch);
    privileged_allowed(|| Ok(lab::is_privileged(&lab::load(dir)?)))?;
    // A lab runs in one place: starting it here (or on another host) while its run is still up
    // elsewhere would drop the marker naming that place, and with it the way to stop it (a cloud
    // lab would keep billing).
    if let Some(up) = super::status::still_up(dirs, id)?
        && up.host.as_deref() != host
    {
        return Err(super::status::still_running(&up));
    }
    if runtime == Runtime::Cloud {
        return cloud_services::start(dirs, lab, host, env, &mut log).await;
    }
    match place {
        Place::Here(run) => start_here(dirs, lab, run, docker_context, inputs, &mut log).await,
        // On a cloud account the per-machine form is cloud-vm (one instance per machine). The
        // dispatch on the provider is `start_on_server`'s to make (the connection).
        Place::Host(host) => start_on_server(dirs, lab, (host, per_machine_vm), inputs, &mut log).await,
    }
}

/// Refuses a start this computer's policy turns off: on a cloud account (`DisableCloudLaunches`;
/// a cloud-services lab always runs on one), or of a lab with a privileged machine
/// (`RefusePrivilegedLabs`; `privileged` reads the lab's spec, only when that switch is on).
pub(crate) fn policy_allows(dirs: &AppDirs, runtime: Runtime, host: Option<&str>, privileged: impl FnOnce() -> Result<bool>) -> Result<()> {
    cloud_allowed(dirs, runtime, host)?;
    privileged_allowed(privileged)
}

/// [`policy_allows`]' first half: refused under `DisableCloudLaunches` when the start runs on a
/// cloud account. Needs no spec, so it is asked before the download ([`check_destination`]).
fn cloud_allowed(dirs: &AppDirs, runtime: Runtime, host: Option<&str>) -> Result<()> {
    let policy = crate::platform::policy::current();
    if policy.is_on(Key::DisableCloudLaunches) && on_cloud(dirs, runtime, host)? {
        return policy.check(Key::DisableCloudLaunches);
    }
    Ok(())
}

/// [`policy_allows`]' second half: refused under `RefusePrivilegedLabs` when `privileged` (the
/// lab's spec, read only when that switch is on) says a machine is privileged.
fn privileged_allowed(privileged: impl FnOnce() -> Result<bool>) -> Result<()> {
    let policy = crate::platform::policy::current();
    if policy.is_on(Key::RefusePrivilegedLabs) && privileged()? {
        return policy.check(Key::RefusePrivilegedLabs);
    }
    Ok(())
}

/// Whether a start of a `runtime` lab on `host` (none: this machine) runs on a cloud account.
fn on_cloud(dirs: &AppDirs, runtime: Runtime, host: Option<&str>) -> Result<bool> {
    let host_is_cloud = match host {
        Some(id) => server::host_provider(dirs, id)?.is_some_and(Provider::is_cloud),
        None => false,
    };
    Ok(runtime == Runtime::Cloud || host_is_cloud)
}

/// [`start_locked`] under the lab lock, for tests that start an installed lab directly.
#[cfg(test)]
pub(crate) async fn start(dirs: &AppDirs, lab: LabRef<'_>, to: Destination<'_>, env: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    let _lock = super::inflight::lock_lab(dirs, lab.id).await?;
    start_locked(dirs, lab, to, env, log).await
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
    inputs: Inputs<'_>,
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
    // was and leftovers are cleared first (the runner's `Occupied::ResumeParked`). What it
    // downloads is the caller's to record ([`downloads`]).
    let home = runner_home(dirs)?;
    let started = match run {
        LocalRun::Docker => start_docker_run(&home, dir, id, inputs, log).await,
        LocalRun::DockerInVm(p) => start_docker_in_vm_run(&home, dir, id, VmStart { provider: p, inputs }, log).await,
        LocalRun::Vm(p) => start_vm_run(&home, dir, id, VmStart { provider: p, inputs }, log).await,
    };
    // A clash is on a host port either form binds here, so name what holds it: the player can
    // then stop it and start again, rather than guess what "port 8080 is taken" means.
    match started {
        Err(e) => Err(port_holder::with_holder(e).await),
        ok => ok,
    }
}

/// The runner's start of a container lab in one VM (Isoloom's `docker-vm`, runner home `home`) on
/// the local hypervisor: its files generated, then up. Takes no app handle, so the integration
/// tests run exactly this.
pub(crate) async fn start_docker_in_vm_run(home: &Path, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let VmStart { provider, inputs } = start;
    let prepared = lab::prepare(dir, lab::vagrant_target(Runtime::Docker))?;
    check_provider(&prepared, lab::Target::DockerVm, provider)?;
    mark_local_vm(dir, Some(provider))?;
    log(super::deploy_log::in_local_vm(provider.id()));
    if provider == Provider::Utm {
        check_utm(log).await?;
    }
    up_in_vm(dir, provider, inputs, || isothrum::LabRun::docker_in_vm(home, dir, id), log).await
}

/// Brings up a container lab's helper VM on `provider` (the run [`isothrum::LabRun`] `run_of`
/// opens) with the launch's `inputs`: on UTM its passing `-600` refusal goes on as
/// [`isothrum::LabRun::start`] says, and a start that failed takes down what it half created.
pub(in crate::runtime) async fn up_in_vm(
    dir: &Path,
    provider: Provider,
    inputs: Inputs<'_>,
    run_of: impl Fn() -> Result<isothrum::LabRun>,
    log: &mut impl FnMut(String),
) -> Result<()> {
    let run = run_of()?;
    // Marked before the start: status, stop and the attack box read the lab as in this VM.
    mark_local_vm(dir, Some(provider))?;
    let Err(e) = run.start(inputs, provider.id(), log).await else {
        return mark_parked(dir, None);
    };
    // A failed start can leave a half-created helper VM holding host ports (e.g. the QEMU
    // SSH forward), and that lingering VM trips the next launch with a port-collision
    // error. Destroy it before surfacing the failure; the teardown is best-effort, so its
    // own error is logged, never allowed to mask the start's.
    log("Start failed: removing the half-created VM so the next launch starts clean".into());
    let teardown = match run_of() {
        Ok(vm) => super::lifecycle::stop_run(vm, dir, &mut *log).await,
        Err(e) => Err(e),
    };
    if let Err(teardown) = teardown {
        log(format!("Could not fully remove the failed VM: {teardown}"));
    }
    Err(e)
}

/// The runner's start of a container lab (runner home `home`): its files generated (the run
/// manifest with them), then up, or resumed as it was when it is parked (the runner's own rule).
/// Takes no app handle, so the integration tests run exactly this.
pub(crate) async fn start_docker_run(home: &Path, dir: &Path, id: &str, inputs: Inputs<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    recheck_instance(dir, id, log).await?;
    lab::prepare(dir, lab::Target::Docker)?;
    mark_local_vm(dir, None)?;
    isothrum::LabRun::docker(home, dir, id)?.up(inputs, None, log).await?;
    mark_parked(dir, None)
}

/// What a VM lab on this machine starts with: its hypervisor, and the launch's values.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VmStart<'a> {
    /// The local hypervisor (Vagrant provider).
    pub(crate) provider: Provider,
    /// The launch's inputs.
    pub(crate) inputs: Inputs<'a>,
}

/// The runner's start of a VM lab (runner home `home`) on `provider`: its files generated (the
/// run manifest with them), the memory warning and QEMU's emulator check, then up (a parked run
/// is resumed as it was). The learner's attack box is added to the lab afterwards, from the lab
/// page or with the lab (`attack_machine`), not by the launch. Takes no app handle, so the
/// integration tests run exactly this.
pub(crate) async fn start_vm_run(home: &Path, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let VmStart { provider, inputs } = start;
    let prepared = lab::prepare(dir, lab::Target::Vagrant)?;
    // A container lab's VM form leaves no local-VM mark behind: that one names docker-vm.
    mark_local_vm(dir, None)?;
    check_provider(&prepared, lab::Target::Vagrant, provider)?;
    warn_if_low_memory(&prepared.spec, log);
    if provider == Provider::Qemu {
        check_qemu(&prepared, log).await?;
    }
    isothrum::LabRun::vm(home, dir, id)?.start(inputs, provider.id(), log).await?;
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

/// Whether a `runtime` lab can start at `to` at all, asked before anything is downloaded for it
/// (see [`destination`], the one rule [`start_locked`] applies too).
pub(crate) fn check_destination(dirs: &AppDirs, runtime: Runtime, to: &Destination<'_>) -> Result<()> {
    destination(dirs, runtime, to).map(drop)
}

/// Where a checked start runs.
#[derive(Debug, PartialEq)]
enum Place<'a> {
    /// On this machine, in this form.
    Here(LocalRun),
    /// On this saved server host or cloud account.
    Host(&'a str),
}

/// Where a `runtime` lab starts at `to`, or why it can't, from what is known before the lab is
/// downloaded: the policy's `DisableCloudLaunches` ([`cloud_allowed`]); on this machine, a form
/// that runs here ([`local_run`]: a cloud-services lab needs a cloud account, a VM lab a
/// hypervisor here, one VM per machine a hypervisor here, ESXi or Proxmox a host); on a host, one
/// VM per machine only on a cloud account ([`per_machine_runs_on`]). The codes are said in the
/// player's language.
fn destination<'a>(dirs: &AppDirs, runtime: Runtime, to: &Destination<'a>) -> Result<Place<'a>> {
    cloud_allowed(dirs, runtime, to.host)?;
    match to.host {
        None => local_run(runtime, to.provider, to.per_machine_vm).map(Place::Here),
        Some(host) => {
            if to.per_machine_vm {
                per_machine_runs_on(server::host_provider(dirs, host)?)?;
            }
            Ok(Place::Host(host))
        }
    }
}

/// The per-machine form runs on this machine or a cloud account, not on a server.
pub(super) const PER_MACHINE_NOT_ON_SERVERS: &str =
    "One VM per machine runs on this machine or a cloud account: on a server (ESXi, Proxmox), the lab runs as it is published.";

/// What a start downloads to this machine, for the layer above to record as the app's (Storage
/// cleans what the app downloaded): container images, Vagrant boxes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Downloads {
    /// Container images, pulled by this machine's Docker.
    pub(crate) images: bool,
    /// Vagrant boxes, added to this machine's Vagrant.
    pub(crate) boxes: bool,
}

/// What a start of a `runtime` lab `to` its destination downloads here: images for containers on
/// this machine's Docker, boxes for every VM form here, nothing for a server or a cloud account
/// (nor for a start [`local_run`] refuses).
pub(crate) fn downloads(runtime: Runtime, to: &Destination<'_>) -> Downloads {
    if to.host.is_some() {
        return Downloads::default();
    }
    match local_run(runtime, to.provider, to.per_machine_vm) {
        Ok(LocalRun::Docker) => Downloads { images: true, boxes: false },
        Ok(LocalRun::DockerInVm(_) | LocalRun::Vm(_)) => Downloads { images: false, boxes: true },
        Err(_) => Downloads::default(),
    }
}

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
            let provider = provider.filter(|p| !p.is_remote()).ok_or_else(|| Error::Refused("One VM per machine needs a hypervisor on this machine".into()))?;
            Ok(LocalRun::Vm(provider))
        }
        Runtime::Docker => Ok(match provider.filter(|p| !p.is_remote()) {
            Some(p) => LocalRun::DockerInVm(p),
            None => LocalRun::Docker,
        }),
        Runtime::Vm => {
            let provider = provider.ok_or_else(|| Error::coded(Code::NoHypervisor))?;
            if provider.is_remote() {
                return Err(Error::Refused("pick a server host to run on ESXi or Proxmox".into()));
            }
            Ok(LocalRun::Vm(provider))
        }
        Runtime::Cloud => Err(Error::coded(Code::PickCloudAccount)),
    }
}

#[cfg(test)]
mod tests;
