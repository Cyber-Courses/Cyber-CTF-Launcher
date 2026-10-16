//! The runner's view of a run, in the lab page's shapes: its status as a `LabStatus`, its check
//! report as the launcher's `Check`. Roles, addresses, ports and networks come from the manifest
//! (through the runner's status); nothing is scraped from container labels.

use isoloom_manifest::{Manifest, OutputRole, Protocol, Role};
use isothrum_runner::{CheckReport, MachineState, MachineStatus, RunState, Status};

use super::super::docker::{Check, CheckResult};
use super::super::{Interface, LabStatus, Machine, Network, Park, Place, Port, Service};

/// The run as the lab page shows it. Running while any of its target machines runs (a lab whose
/// one-shot jobs finished is up); parked as the player chose (`marked`, the lab's mark), else
/// shut down; its URL is its entry output, once it runs.
pub(super) fn lab_status(manifest: &Manifest, status: &Status, marked: Option<Park>) -> LabStatus {
    let running = status.machines.iter().any(|m| m.role == Role::Target && matches!(m.state, MachineState::Running | MachineState::Unhealthy));
    let url = status.outputs.iter().find(|o| o.role == OutputRole::Entry && !o.sensitive).and_then(|o| o.value.clone()).filter(|_| running);
    LabStatus {
        running,
        parked: (status.state == RunState::Parked && !running).then(|| marked.unwrap_or(Park::Shutdown)),
        machines: status.machines.iter().filter(|m| m.state != MachineState::Absent).map(|m| machine(manifest, m)).collect(),
        networks: status.networks.iter().map(|n| Network { name: n.name.clone(), subnet: n.cidr.clone(), internal: !n.internet }).collect(),
        url,
        place: Some(Place::Container),
        ..LabStatus::default()
    }
}

/// One machine: its state as Docker words it, its addresses, its TCP ports with the host port
/// each got (0: not published), and what it serves. Isoloom's own machines (jobs, probes, route
/// keepers, the access stand-in, observers) are plumbing: hidden from the diagram, never "down".
fn machine(manifest: &Manifest, m: &MachineStatus) -> Machine {
    let declared = manifest.machines.iter().find(|d| d.name.as_str() == m.name).map(|d| d.ports.as_slice()).unwrap_or_default();
    let interfaces: Vec<Interface> = m.addresses.iter().map(|(network, ip)| Interface { network: network.clone(), ip: ip.clone() }).collect();
    Machine {
        name: m.name.clone(),
        state: state_word(&m.state),
        image: String::new(),
        ip: interfaces.first().map(|i| i.ip.clone()).unwrap_or_default(),
        ports: declared.iter().filter(|p| p.kind.transport() == Protocol::Tcp).map(|p| tcp_port(m, p.port.get())).collect(),
        interfaces,
        services: declared.iter().map(service).collect(),
        infra: m.role != Role::Target,
    }
}

/// `port` and the host port it is published on (0 when it isn't).
fn tcp_port(m: &MachineStatus, port: u16) -> Port {
    let published = m.published.iter().find(|p| p.port == port && p.protocol == Protocol::Tcp).map_or(0, |p| p.host_port);
    Port { published, target: port }
}

/// What a declared port serves: a web UI (`http`), else the port's own name (`mysql`, `ssh`), else
/// its number.
fn service(p: &isoloom_manifest::Port) -> Service {
    let name = p.name.as_str().to_owned();
    Service { kind: if p.kind.http() { "web".into() } else { name.clone() }, name, ports: vec![p.port.get()] }
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

/// A lab that declares no checks: nothing to verify.
pub(super) fn no_checks() -> Check {
    Check { available: false, ok: false, output: String::new(), results: Vec::new() }
}

/// The runner's check report, with the runners' output (`lines`, as they printed it).
pub(super) fn check(report: &CheckReport, lines: &[String]) -> Check {
    // A check its runner never reported is in the report already, failed.
    let results = report.results.iter().map(|r| CheckResult { name: r.name.clone(), from: r.from.clone(), ok: r.ok, reason: r.detail.clone() }).collect();
    Check { available: true, ok: report.ok, output: lines.join("\n"), results }
}
