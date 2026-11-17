//! Starting a lab: on this machine (local Docker, a container lab in one local VM, or one VM per
//! machine), or on a server host (ESXi through Vagrant, Proxmox and the clouds through Terraform).

use std::path::Path;

use crate::platform::AppDirs;

#[cfg(feature = "cloud-services")]
use super::cloud_services;
pub(crate) use super::isothrum::Inputs;
use super::paths::runner_home;
use super::placement::Placement;
use super::providers::Provider;
use super::{LabRef, Runtime, isothrum, lab, server};
use crate::error::{Code, Error, Result};
use crate::platform::policy::Key;

mod local;
mod on_host;

pub(crate) use local::check_utm;
#[cfg(test)]
use local::low_memory_warning;
use local::{check_provider, check_qemu, warn_if_low_memory};
#[cfg(feature = "cloud-services")]
#[cfg(feature = "cloud-services")]
pub(super) use on_host::check_aws_budget;
pub(in crate::runtime) use on_host::esxi_run;
#[cfg(any(test, feature = "cloud-services"))]
pub(super) use on_host::public_ip;
pub(super) use on_host::server_place;
#[cfg(test)]
pub(crate) use on_host::{CloudStart, start_cloud_run};
#[cfg(test)]
use on_host::{budget_verdict, cloud_form, ip_unsure};
use on_host::{containers_run_on, per_machine_runs_on, start_on_server};

/// Where a lab is started ([`Placement`], as picked) and the Docker context a container lab here
/// is pinned to (resolved from the placement's engine on the main process, where Docker answers).
#[derive(Clone, Copy)]
pub(crate) struct Destination<'a> {
    /// Where and how it runs.
    pub(crate) placement: &'a Placement,
    /// The Docker context a container lab here runs on (the learner's engine); none: the
    /// current one.
    pub(crate) docker_context: Option<&'a str>,
}

impl<'a> Destination<'a> {
    /// `placement`, on the current Docker context.
    pub(crate) fn at(placement: &'a Placement) -> Destination<'a> {
        Destination { placement, docker_context: None }
    }

    /// The saved server or cloud account it starts on; none for this machine.
    fn host(&self) -> Option<&'a str> {
        self.placement.host()
    }
}

/// A [`Destination`] that owns its values: what a prepared launch carries to its run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    /// [`Destination::placement`].
    pub(crate) placement: Placement,
    /// [`Destination::docker_context`].
    pub(crate) docker_context: Option<String>,
}

impl Placed {
    /// Where the lab starts, borrowed.
    pub(crate) fn destination(&self) -> Destination<'_> {
        Destination { placement: &self.placement, docker_context: self.docker_context.as_deref() }
    }
}

/// Starts an installed lab; `env` holds the lab's input values (the evidence claim, the player's
/// flags).
///
/// - `Docker`: a container lab's containers on this machine's Docker.
/// - `Vms`: the lab's VMs here, one per machine (Isoloom's vagrant): a VM lab, or a container
///   lab's VM recipes.
/// - `Server`: a VM lab there. ESXi runs the same Vagrantfiles; Proxmox Isoloom's Terraform.
/// - `Cloud`: the lab's cloud form there (Docker on one VM, or one VM per machine); a
///   cloud-services lab (`Runtime::Cloud`) runs only there.
///
/// One start of a lab at a time is the run service's to hold (`run_service::LocalRunService`),
/// the runner service refusing a second operation on its run. A start with nowhere to run, or that the administrator's policy turns off, is refused before anything
/// changes ([`destination`], then [`privileged_allowed`]).
pub(crate) async fn start_locked(dirs: &AppDirs, lab: LabRef<'_>, to: Destination<'_>, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    let LabRef { dir, id, runtime } = lab;
    let place = destination(dirs, runtime, &to)?;
    let (host, docker_context) = (to.host(), to.docker_context);
    let inputs = Inputs { values: env, default_ports: to.placement.default_ports(), docker_context: None };
    privileged_allowed(|| Ok(lab::is_privileged(&lab::load(dir)?)))?;
    // A lab runs in one place: starting it here (or on another host) while its run is still up
    // elsewhere would drop the marker naming that place, and with it the way to stop it (a cloud
    // lab would keep billing). A run here taken down outside the app is forgotten first.
    super::status::forget_gone(dirs, id, &mut log).await?;
    if let Some(up) = super::status::still_up(dirs, id).await?
        && up.host.as_deref() != host
    {
        return Err(super::status::still_running(&up));
    }
    if runtime == Runtime::Cloud {
        #[cfg(feature = "cloud-services")]
        return cloud_services::start(dirs, lab, host, env, &mut log).await;
        #[cfg(not(feature = "cloud-services"))]
        return Err(Error::Refused(super::CLOUD_UNSUPPORTED.into()));
    }
    match place {
        Place::Here(run) => start_here(dirs, lab, run, docker_context, inputs, &mut log).await,
        // On a cloud account the per-machine form is cloud-vm (one instance per machine). The
        // dispatch on the provider is `start_on_server`'s to make (the connection).
        Place::Host(host, per_machine) => start_on_server(dirs, lab, (host, per_machine), inputs, &mut log).await,
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

/// [`start_locked`], for tests that start an installed lab directly.
#[cfg(test)]
pub(crate) async fn start(dirs: &AppDirs, lab: LabRef<'_>, to: Destination<'_>, env: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    start_locked(dirs, lab, to, env, log).await
}

/// Starts the lab on this machine as `run` says (see [`destination`]).
#[expect(
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
    let LabRef { dir, id, .. } = lab;
    // The run's record keeps the form it was started in (its project) and where (no host): every
    // later operation on it (status, stop, park) follows it, whatever the catalogue calls it.
    // The container engine the player chose for a plain container lab (a Docker context other than
    // the current one): the runner records it with the run, and every later docker call of the run
    // goes there. Only containers on Docker here (not a VM form); none = the current context.
    let inputs = Inputs { docker_context: docker_context.filter(|_| matches!(run, LocalRun::Docker)), ..inputs };
    // Every lab on this machine runs through the Isothrum runner. A parked run is resumed as it
    // was and leftovers are cleared first (the runner's `Occupied::ResumeParked`). What it
    // downloads is the caller's to record ([`downloads`]).
    let home = runner_home(dirs)?;
    // A clash on a host port either form binds here names what holds it (the runner's lookup).
    match run {
        LocalRun::Docker => start_docker_run(&home, dir, id, inputs, log).await,
        LocalRun::Vm(p) => start_vm_run(&home, dir, id, VmStart { provider: p, inputs }, log).await,
    }
}

/// The runner's start of a container lab (runner home `home`): its files generated, then up, or resumed as it was when it is parked (the runner's own rule).
/// Takes no app handle, so the integration tests run exactly this.
pub(crate) async fn start_docker_run(home: &Path, dir: &Path, id: &str, inputs: Inputs<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    recheck_instance(home, dir, id, log).await?;
    lab::prepare(dir, lab::Target::Docker)?;
    isothrum::Deployment::docker(home, dir, id)?.up(inputs, None, log).await
}

/// What a VM lab on this machine starts with: its hypervisor, and the launch's values.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VmStart<'a> {
    /// The local hypervisor (Vagrant provider).
    pub(crate) provider: Provider,
    /// The launch's inputs.
    pub(crate) inputs: Inputs<'a>,
}

/// The runner's start of a VM lab (runner home `home`) on `provider`: its files generated, the memory
/// warning, QEMU's emulator check and UTM open and answering
/// ([`check_utm`]: Vagrant's UTM plugin drives it with Apple events), then up (a parked run
/// is resumed as it was). The learner's attack box is added to the lab afterwards, from the lab
/// page or with the lab (`attack_box`), not by the launch. Takes no app handle, so the
/// integration tests run exactly this.
pub(crate) async fn start_vm_run(home: &Path, dir: &Path, id: &str, start: VmStart<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let VmStart { provider, inputs } = start;
    let prepared = lab::prepare(dir, lab::Target::Vagrant)?;
    check_provider(&prepared.lab, lab::Target::Vagrant, provider)?;
    warn_if_low_memory(&prepared.spec, log);
    if provider == Provider::Qemu {
        check_qemu(&prepared, log).await?;
    }
    if provider == Provider::Utm {
        check_utm(log).await?;
    }
    isothrum::Deployment::vm(home, dir, id)?.up(inputs, Some(provider.id()), log).await
}

/// Why the launcher refuses to start the lab installed in `dir` as `target` (Isoloom's `vagrant`)
/// on the local hypervisor `provider`, before anything runs: the refusal [`start_vm_run`] makes,
/// read from the lab's spec alone (nothing generated). None when it would start there. What the integration tests skip on, saying why.
#[cfg(test)]
pub(crate) fn refusal_here(dir: &Path, target: lab::Target, provider: Provider) -> Result<Option<Error>> {
    Ok(check_provider(&lab::lab(dir)?, target, provider).err())
}

/// Checks the lab's instance against the networks Docker has now (an install with Docker down
/// picked it blind), moving the lab (stopped: its slot was cleared) to a free one if its blocks
/// are taken. With Docker down there is nothing to check against, and the start says so next.
async fn recheck_instance(home: &Path, dir: &Path, id: &str, log: &mut impl FnMut(String)) -> Result<()> {
    let Some(labs) = dir.parent() else { return Ok(()) };
    let before = lab::instance(dir);
    let now = lab::recheck_instance(labs, dir, &isothrum::address_blocks_in_use(home, id).await)?;
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
    /// On this saved server host or cloud account; one VM per machine there.
    Host(&'a str, bool),
}

/// Where a `runtime` lab starts at `to`, as its placement says, or why it can't, from what is
/// known before the lab is downloaded: the policy's `DisableCloudLaunches` ([`cloud_allowed`]);
/// a placement whose kind the lab has no form for (a cloud-services lab needs a cloud account, a
/// VM lab VMs, a hypervisor here a local one); a host of the kind the placement says (a server
/// runs a VM lab, a cloud account its cloud forms). The placement is taken as it is: nothing is
/// filled in or reinterpreted. The codes are said in the player's language.
fn destination<'a>(dirs: &AppDirs, runtime: Runtime, to: &Destination<'a>) -> Result<Place<'a>> {
    cloud_allowed(dirs, runtime, to.host())?;
    if runtime == Runtime::Cloud && !matches!(to.placement, Placement::Cloud { .. }) {
        return Err(Error::coded(Code::PickCloudAccount));
    }
    match to.placement {
        Placement::Docker { .. } if runtime == Runtime::Vm => Err(Error::coded(Code::NoHypervisor)),
        Placement::Docker { .. } => Ok(Place::Here(LocalRun::Docker)),
        Placement::Vms { provider } if provider.is_remote() => Err(Error::Refused("pick a server host to run on ESXi or Proxmox".into())),
        Placement::Vms { provider } => Ok(Place::Here(LocalRun::Vm(*provider))),
        Placement::Server { host } => {
            let kind = server::host_provider(dirs, host)?;
            if kind.is_some_and(Provider::is_cloud) {
                return Err(Error::Refused("this host is a cloud account, not a server".into()));
            }
            if runtime == Runtime::Docker {
                containers_run_on(kind)?;
            }
            Ok(Place::Host(host, false))
        }
        Placement::Cloud { account, per_machine } => {
            let kind = server::host_provider(dirs, account)?;
            if *per_machine {
                per_machine_runs_on(kind)?;
            } else {
                containers_run_on(kind)?;
            }
            Ok(Place::Host(account, *per_machine))
        }
    }
}

/// The per-machine form runs on this machine or a cloud account, not on a server.
pub(crate) const PER_MACHINE_NOT_ON_SERVERS: &str =
    "One VM per machine runs on this machine or a cloud account: on a server (ESXi, Proxmox), the lab runs as it is published.";

/// A container lab's containers run on this machine's Docker or on a cloud account's VM, not on a
/// server, which runs VMs only.
pub(crate) const CONTAINERS_NOT_ON_SERVERS: &str =
    "A container lab runs on this machine (on Docker, or as one VM per machine) or in a cloud account, not on a server (ESXi, Proxmox).";

/// How a lab runs on this machine.
#[derive(Debug, PartialEq)]
enum LocalRun {
    /// A container lab on this machine's Docker.
    Docker,
    /// The lab's VMs, one per machine on this hypervisor (a VM lab, or a container lab's VM form).
    Vm(Provider),
}

#[cfg(test)]
mod tests;
