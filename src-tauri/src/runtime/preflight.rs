//! Everything on this machine that would stop a start, checked before the launch is registered
//! (`startLab` spends a launch token): the policy, the placement against what Isoloom allows, the
//! hypervisor's readiness, the container engine and its Docker context, and the host ports the
//! lab's spec keeps fixed (what holds one that is taken, named).
//! Every problem is found, not only the first; [`verdict`] is the one mapper that says them, as
//! the coded errors a start says.

use isothrum_runner::Protocol;

use crate::error::{Code, Error, Result};
use crate::platform::AppDirs;

use super::lab::{self, Lab, RunOptions};
use super::placement::{self, Placement};
use super::providers::Provider;
use super::{Runtime, server};

/// What a launch of lab `lab_id`, declared `runtime`, at `placement` finds wrong here, from its
/// spec at `source` ([`verdict`]: every problem at once).
pub(crate) async fn check(dirs: &AppDirs, lab_id: &str, runtime: Runtime, source: lab::Source<'_>, placement: &Placement) -> Result<()> {
    // A cloud-services lab has no machines: its options come from its lab file.
    let (lab, options) = if runtime == Runtime::Cloud {
        (None, Box::pin(placement::options_at(dirs, lab_id, runtime, source)).await?)
    } else {
        let lab = Box::pin(lab::lab_at(dirs, lab_id, source)).await?;
        let options = lab::run_options(&lab, runtime);
        (Some(lab), options)
    };
    let present = super::lab_present(dirs, lab_id).await;
    verdict(Box::pin(problems(dirs, &Checked { runtime, lab: lab.as_ref(), options: &options, placement, present })).await)
}

/// What one launch is checked against.
struct Checked<'a> {
    /// The runtime the lab declares.
    runtime: Runtime,
    /// Its spec (none for a cloud-services lab).
    lab: Option<&'a Lab>,
    /// Every way Isoloom (and this computer's policy) lets it start.
    options: &'a RunOptions,
    /// Where it is to start.
    placement: &'a Placement,
    /// Something of it is here already (a parked run keeps its host ports).
    present: bool,
}

/// Every problem `c` has, in the order a player puts them right.
async fn problems(dirs: &AppDirs, c: &Checked<'_>) -> Vec<Error> {
    let mut found = Vec::new();
    let privileged = c.lab.is_some_and(|l| lab::is_privileged(&l.spec()));
    // The policy first: the options hold its refusals too, which would say it twice.
    let allowed = super::policy_allows(dirs, c.runtime, c.placement.host(), || Ok(privileged))
        .and_then(|()| placement::allowed(c.runtime, c.options, c.placement, |id| server::host_provider(dirs, id)));
    found.extend(allowed.err());
    match c.placement {
        Placement::Vms { provider } => found.extend(hypervisor_ready(*provider, c.lab).await.err()),
        // Containers here, for a container lab (any other is refused above).
        Placement::Docker { engine, .. } if c.runtime == Runtime::Docker => {
            found.extend(engine_answers(engine.map(crate::machine::system::docker::Engine::id)).await.err());
            if !c.present
                && let Some(lab) = c.lab
            {
                found.extend(taken_ports(&fixed_ports(lab)).await);
            }
        }
        Placement::Docker { .. } | Placement::Server { .. } | Placement::Cloud { .. } => {}
    }
    found
}

/// The one mapper: nothing wrong, the launch goes on; one problem, its own coded error; several,
/// each said, one a line, so the player puts them all right before trying again.
pub(crate) fn verdict(problems: Vec<Error>) -> Result<()> {
    let mut problems = problems;
    match problems.len() {
        0 => Ok(()),
        1 => Err(problems.remove(0)),
        _ => Err(Error::Refused(format!(
            "This lab can't start here yet:\n{}",
            problems.iter().map(|p| format!("• {}", p.to_wire().message)).collect::<Vec<_>>().join("\n")
        ))),
    }
}

/// `provider` ready here for the lab's VMs, as isothrum-machine's check says (Vagrant, the
/// hypervisor and its plugin), and, for VMs built for another CPU, its emulator (QEMU's).
async fn hypervisor_ready(provider: Provider, lab: Option<&Lab>) -> Result<()> {
    let cx = super::providers::machine();
    let Some(vagrant) = provider.vagrant() else { return Ok(()) };
    let installed = isothrum_machine::tools::version(&cx, isothrum_machine::tools::probes::VAGRANT).await.installed;
    let status = isothrum_machine::providers::status(&cx, vagrant).await;
    let ready = status.as_ref().is_some_and(|s| !isothrum_machine::check::ready(std::slice::from_ref(s), installed).is_empty());
    if !installed {
        return Err(Error::ToolMissing { tool: "Vagrant" });
    }
    if !ready {
        let why = status.and_then(|s| s.reason).map(|r| super::providers::said(&r));
        return Err(Error::Refused(match why {
            Some(why) => format!("{} isn't ready on this machine: {why}", provider.label()),
            None => format!("{} isn't ready on this machine: set it up from the Machine page.", provider.label()),
        }));
    }
    if provider == Provider::Qemu
        && let Some(lab) = lab
    {
        let arches = lab::emulated_arches(&lab.spec(), cx.host.arch);
        if let Err(isothrum_machine::providers::ready::NotReady::Emulator { program, .. }) = isothrum_machine::providers::ready::emulators(&cx, arches).await {
            return Err(Error::ToolMissing { tool: program });
        }
    }
    Ok(())
}

/// The container engine answers: its Docker context found (`engine`; none: the current one), and
/// the daemon there answering this account.
async fn engine_answers(engine: Option<&str>) -> Result<()> {
    let context = match engine {
        Some(engine) => Some(crate::machine::system::docker::engine_context(engine).await?),
        None => None,
    };
    let cx = super::providers::machine();
    isothrum_machine::docker::daemon_info(&cx, "{{.ServerVersion}}", context.as_deref()).await.map(drop).map_err(crate::machine::system::docker::no_answer)
}

/// The host ports the lab's spec keeps fixed (`publish` with `fixed: true`: apps whose pages call
/// `localhost:<that port>` themselves), each with its protocol. Every other published port is a
/// free one the runner picks at the start.
fn fixed_ports(lab: &Lab) -> Vec<(u16, Protocol)> {
    let mut ports: Vec<(u16, Protocol)> = lab
        .spec()
        .machines
        .values()
        .flat_map(|m| m.ports.iter())
        .filter(|p| p.fixed)
        .filter_map(|p| Some((p.publish?, if p.protocol.is_tcp() { Protocol::Tcp } else { Protocol::Udp })))
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Of `ports`, the ones something on this machine holds (isothrum-machine's `net::port_free`), as
/// the start would say each: naming the process that holds it when the system's tools say
/// (`net::port_holder`), as a run's own taken port is named (`isothrum::errors::holder_named`).
async fn taken_ports(ports: &[(u16, Protocol)]) -> Vec<Error> {
    let cx = super::providers::machine();
    let mut taken = Vec::new();
    for &(port, protocol) in ports.iter().filter(|(port, protocol)| !isothrum_machine::net::port_free(*port, *protocol)) {
        let holder = isothrum_machine::net::port_holder(&cx, port, protocol).await;
        let holder = holder.as_ref().map(super::isothrum::errors::holder_named);
        taken.push(Error::coded(Code::PortInUse { port: isothrum_runner::HostPort::new(port), holder }));
    }
    taken
}

#[cfg(test)]
mod tests;
