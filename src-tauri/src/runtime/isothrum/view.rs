//! The runner's view of a run, in the lab page's shapes: its status as a `LabStatus`, its checks'
//! output as the launcher's `Check`. What a machine is (its ports, addresses, networks, whether it
//! is the lab's or Isoloom's plumbing) comes from the lab's spec (`described`); what the runner
//! adds is what its tool sees: each machine's state and the host ports it got.

use isothrum_runner::{MachineState, MachineStatus, Protocol, RunState, Status};

use super::super::docker::{Check, CheckResult};
use super::super::port::{GuestPort, HostPort};
use super::super::{LabStatus, Machine, Park, Place, Port, Service};
use super::described::{Declared, DeclaredPort, Described};

/// The run as the lab page shows it. Running while any of the lab's machines runs (a lab whose
/// one-shot jobs finished is up); parked as the player chose (`marked`, the lab's mark), else
/// shut down; its URL is the page of its first published web port, once it runs. Its machines
/// are those there; while it runs, a machine of the lab that is gone (removed outside the app)
/// is shown too, `absent`, so the page says it is down and offers a way back rather than
/// drawing the lab without it.
pub(super) fn lab_status(lab: &Described, status: &Status, marked: Option<Park>) -> LabStatus {
    let running = runs_in(lab, status);
    let native = crate::screen::native::installed_here(status.native.as_ref());
    let shown = |m: &&MachineStatus| m.state != MachineState::Absent || (running && lab.machine(&m.name).is_some());
    LabStatus {
        running,
        parked: (status.state == RunState::Parked && !running).then(|| marked.unwrap_or(Park::Shutdown)),
        machines: status.machines.iter().filter(shown).map(|m| machine(lab.machine(&m.name), m, native.as_deref())).collect(),
        networks: lab.networks.clone(),
        url: entry(lab, status).filter(|_| running),
        place: Some(Place::Container),
        // The hypervisor (or engine) the run is on, as the runner recorded it: the page names it,
        // and offers Pause only where that provider can suspend (vagrant-qemu can't). A lab on a
        // server or cloud account has its provider set by its own run.
        provider: status.provider.map(|p| isothrum_runner::wire_name(&p)),
        ..LabStatus::default()
    }
}

/// Whether any of the run's machines runs (an unhealthy one runs too), Isoloom's own included:
/// what a run with no description to read (the helper VM an earlier version left) goes by.
pub(super) fn runs(status: &Status) -> bool {
    status.machines.iter().any(|m| matches!(m.state, MachineState::Running | MachineState::Unhealthy))
}

/// Whether any of the lab's own machines runs.
fn runs_in(lab: &Described, status: &Status) -> bool {
    if lab.machines.is_empty() {
        return runs(status);
    }
    status.machines.iter().any(|m| lab.machine(&m.name).is_some() && matches!(m.state, MachineState::Running | MachineState::Unhealthy))
}

/// The page the lab starts at: its first web port published on the host, at the host's loopback, else
/// at its machine's own address.
fn entry(lab: &Described, status: &Status) -> Option<String> {
    lab.machines.iter().find_map(|d| {
        let seen = status.machines.iter().find(|m| m.name == d.name)?;
        d.ports.iter().filter(|p| p.web()).find_map(|p| {
            // Published on this computer, else at the machine's own address (a VM nothing forwards
            // to: on an ESXi host, on libvirt's network).
            let at = match published(seen, p) {
                Some(host) => format!("127.0.0.1:{}", host.get()),
                None => format!("{}:{}", seen.address.as_deref()?, p.port),
            };
            Some(format!("{}://{at}/", if p.protocol == "https" { "https" } else { "http" }))
        })
    })
}

/// One machine: its state as Docker words it, its addresses, its TCP ports with the host port
/// each got (0: not published) and what it serves, as `declared` says (none: one of Isoloom's
/// own machines, plumbing hidden from the diagram, never "down"), and its screen (with `native`,
/// the app this computer has to open it in). Its address is the one the runner reads where the
/// lab's spec gives none for the form it runs in.
fn machine(declared: Option<&Declared>, m: &MachineStatus, native: Option<&str>) -> Machine {
    let interfaces = declared.map(|d| d.interfaces.clone()).unwrap_or_default();
    let ports = declared.map(|d| d.ports.as_slice()).unwrap_or_default();
    Machine {
        name: m.name.clone(),
        state: state_word(&m.state),
        ip: interfaces.first().map(|i| i.ip.clone()).or_else(|| m.address.clone()).unwrap_or_default(),
        ports: ports.iter().filter(|p| p.tcp()).map(|p| Port { published: published(m, p), target: GuestPort::new(p.port), login: None }).collect(),
        interfaces,
        services: ports.iter().map(service).collect(),
        infra: declared.is_none(),
        console: m.console.as_ref().map(|c| super::super::Console::of(c, native)),
    }
}

/// The host port `port` is published on (none when it isn't), as the runner reports it: never
/// the machine's own number.
fn published(m: &MachineStatus, port: &DeclaredPort) -> Option<HostPort> {
    let tcp = port.tcp();
    m.published
        .iter()
        .find(|p| p.port.get() == port.port && p.protocol.is_none_or(|proto| (proto == Protocol::Tcp) == tcp))
        .map(|p| HostPort::new(p.host_port.get()))
}

/// What a declared port serves: a web UI (`http`), else the port's own name (`mysql`, `ssh`).
fn service(p: &DeclaredPort) -> Service {
    Service { kind: if p.web() { "web".into() } else { p.name.clone() }, name: p.name.clone(), ports: vec![p.port] }
}

/// A machine's state as the lab page reads it (`running` is what counts as up).
fn state_word(state: &MachineState) -> String {
    match state {
        MachineState::Running => "running".into(),
        MachineState::Unhealthy => "unhealthy".into(),
        MachineState::Exited(_) => "exited".into(),
        MachineState::Stopped => "stopped".into(),
        MachineState::Absent => "absent".into(),
        other => other.to_string(),
    }
}

/// The checks' outcome from what their runners printed (`lines`, each with the runner it came
/// from), by Isoloom's check protocol; failed when a runner failed without saying why.
pub(super) fn check(lines: &[(String, String)], runners_ok: bool) -> Check {
    let results: Vec<CheckResult> = lines
        .iter()
        .filter_map(|(from, line)| {
            let (name, ok, reason) = super::described::check_line(line)?;
            Some(CheckResult { name, from: format!("from {from}"), ok, reason })
        })
        .collect();
    let ok = runners_ok && results.iter().all(|r| r.ok);
    Check { available: true, ok, output: lines.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n"), results }
}
