//! What a lab says of itself, read from its spec through Isoloom (its resolved snapshot), never
//! from a file the runner reads: its machines with their ports and addresses, its networks, the
//! runners of its checks, and its message. The runner reports only what its tool sees (each
//! machine's state, its published ports); the lab page joins the two (`view`).

use std::path::Path;

use serde_json::Value;

use super::super::{Interface, Network, lab};
use crate::error::Result;

/// A lab's own description, as one target runs it.
#[derive(Default)]
pub(super) struct Described {
    /// The lab's machines (Isoloom's own, the controller and the check runners, aren't among
    /// them: they are its plumbing).
    pub(super) machines: Vec<Declared>,
    /// The lab's networks.
    pub(super) networks: Vec<Network>,
    /// Where its checks run: each runner's machine (a Compose service, a VM).
    pub(super) check_runners: Vec<String>,
}

/// One machine of the lab, as its spec declares it.
#[derive(Default)]
pub(super) struct Declared {
    /// Its name.
    pub(super) name: String,
    /// Its ports, by name.
    pub(super) ports: Vec<DeclaredPort>,
    /// Its address on each network it is on, where the target puts it.
    pub(super) interfaces: Vec<Interface>,
}

/// One port of a machine, as its spec declares it.
#[derive(Debug, Clone)]
pub(super) struct DeclaredPort {
    /// The spec's name for it (its number when it gives none).
    pub(super) name: String,
    /// The port the machine listens on.
    pub(super) port: u16,
    /// What it speaks: `tcp`, `udp`, `http`, `https`, `tls`.
    pub(super) protocol: String,
}

impl DeclaredPort {
    /// Whether it serves a web page (`http`, `https`).
    pub(super) fn web(&self) -> bool {
        matches!(self.protocol.as_str(), "http" | "https")
    }

    /// Whether it is reached over TCP (every protocol but `udp`).
    pub(super) fn tcp(&self) -> bool {
        self.protocol != "udp"
    }
}

/// How a target places the lab's machines: at their Docker addresses on the Compose targets, at
/// the spec's own elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Form {
    /// The lab's Compose file.
    Containers,
    /// VMs (Vagrant, a server's or a cloud's module).
    Vms,
}

impl Described {
    /// The lab in `dir` (as its instance) in `form`, from its spec; Vagrant's check runners are
    /// the VMs its generated project installs them in (`checks/<vm>.sh` beside the Vagrantfile).
    pub(super) fn of(dir: &Path, form: Form) -> Result<Described> {
        let snapshot = lab::resolved(dir)?;
        let check_runners = match form {
            Form::Containers => compose_runners(&snapshot),
            Form::Vms => vagrant_runners(&lab::vagrant_dir(dir, super::super::Runtime::Vm)),
        };
        Ok(Described { machines: machines(&snapshot, form), networks: networks(&snapshot), check_runners })
    }

    /// The machine named `name`, when the lab declares it.
    pub(super) fn machine(&self, name: &str) -> Option<&Declared> {
        self.machines.iter().find(|m| m.name == name)
    }
}

/// The snapshot's machines, at their addresses in `form`.
fn machines(snapshot: &Value, form: Form) -> Vec<Declared> {
    let key = if form == Form::Containers { "docker_addresses" } else { "addresses" };
    let Some(machines) = snapshot["machines"].as_object() else { return Vec::new() };
    machines
        .iter()
        .map(|(name, m)| Declared {
            name: name.clone(),
            ports: m["ports"].as_object().map(|ports| ports.iter().filter_map(|(name, p)| port(name, p)).collect()).unwrap_or_default(),
            interfaces: m[key]
                .as_object()
                .map(|a| a.iter().filter_map(|(net, ip)| ip.as_str().map(|ip| Interface { network: net.clone(), ip: ip.to_owned() })).collect())
                .unwrap_or_default(),
        })
        .collect()
}

/// A port of the snapshot, named `name`.
fn port(name: &str, p: &Value) -> Option<DeclaredPort> {
    let port = u16::try_from(p["port"].as_u64()?).ok()?;
    Some(DeclaredPort { name: name.to_owned(), port, protocol: p["protocol"].as_str().unwrap_or("tcp").to_owned() })
}

/// The snapshot's networks.
fn networks(snapshot: &Value) -> Vec<Network> {
    let Some(networks) = snapshot["networks"].as_object() else { return Vec::new() };
    networks
        .iter()
        .map(|(name, n)| Network {
            name: name.clone(),
            subnet: n["cidr"].as_str().unwrap_or_default().to_owned(),
            internal: !n["internet"].as_bool().unwrap_or(true),
        })
        .collect()
}

/// The Compose services the snapshot's check positions run in (`isoloom-check[-<position>]`).
fn compose_runners(snapshot: &Value) -> Vec<String> {
    snapshot["checks"]["positions"]
        .as_array()
        .map(|positions| positions.iter().filter_map(|p| p["runner"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// The VMs of the Vagrant project in `project` that hold a check runner (`checks/<vm>.sh`).
fn vagrant_runners(project: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(project.join("checks")) else { return Vec::new() };
    let mut vms: Vec<String> =
        entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter_map(|name| name.strip_suffix(".sh").map(str::to_owned)).collect();
    vms.sort();
    vms
}

/// The provisioner of Isoloom's Vagrantfiles that runs a VM's checks, its scripts read from the
/// host first (`vagrant provision <vm> --provision-with checks`).
pub(super) const VAGRANT_CHECKS: &str = "checks";

/// What marks a check runner's line (Isoloom's `isoloom-check/1` protocol), wherever it starts
/// in the line its tool prints.
const MARKER: &str = "isoloom-check: ";

/// One line of a check runner's output, read: a check passed (`PASS <name>`), failed (`FAIL
/// <name>: <reason>`); none for any other line (the `END` count included).
pub(super) fn check_line(line: &str) -> Option<(String, bool, String)> {
    let (_, rest) = line.split_once(MARKER)?;
    if let Some(name) = rest.strip_prefix("PASS ") {
        return Some((name.trim_end().to_owned(), true, String::new()));
    }
    let failed = rest.strip_prefix("FAIL ")?;
    let (name, reason) = failed.split_once(": ").unwrap_or((failed, ""));
    Some((name.trim_end().to_owned(), false, reason.trim_end().to_owned()))
}

/// The lab's message (how to start, in Markdown) as it reads on `target`, its placeholders
/// filled from its spec; none when it has none, or when it can't be read (logged).
pub(super) fn message(dir: &Path, target: lab::Target) -> Option<String> {
    lab::message_on(dir, target).unwrap_or_else(|e| {
        log::warn!("{}: the lab's message couldn't be read: {e}", dir.display());
        None
    })
}

/// Logs the lab's message ([`message`]) on `target` line by line, where a lab's run says where to
/// start once it is up; nothing when the run has no target or the lab no message.
pub(super) fn log_message(dir: &Path, target: Option<lab::Target>, log: &mut impl FnMut(String)) {
    let message = target.and_then(|t| message(dir, t));
    message.iter().flat_map(|m| m.lines()).for_each(|l| log(l.to_owned()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two machines on two networks (one cut off from the internet), ports of each protocol, a
    /// check, and a message naming a published port.
    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  front: { cidr: 10.40.0.0/24 }\n  back: { cidr: 10.41.0.0/24, internet: false }\nmachines:\n  web:\n    networks: { front: 10, back: 10 }\n    ports: { site: { port: 80, protocol: http, publish: 8080 }, dns: { port: 53, protocol: udp } }\n    app: { image: nginx:1.27 }\n  db:\n    networks: { back: 20 }\n    ports: { sql: { port: 5432, protocol: tcp } }\n    app: { image: postgres:17 }\nchecks:\n  - { http: \"http://web/\" }\nmessage: |\n  Open http://localhost:{{ machines.web.ports.site.published }}/\n";

    fn lab() -> crate::testing::TempDir {
        crate::runtime::test_support::write_lab(SPEC)
    }

    #[test]
    fn the_spec_says_each_machines_ports_addresses_and_the_networks() {
        let dir = lab();
        let d = Described::of(&dir, Form::Containers).unwrap();
        let web = d.machine("web").unwrap();
        let ports: Vec<(&str, u16, bool, bool)> = web.ports.iter().map(|p| (p.name.as_str(), p.port, p.web(), p.tcp())).collect();
        assert_eq!(ports, [("site", 80, true, true), ("dns", 53, false, false)], "in the spec's order");
        assert_eq!(web.interfaces.iter().map(|i| i.network.as_str()).collect::<Vec<_>>(), ["front", "back"]);
        assert!(d.machine("db").unwrap().ports.iter().all(|p| p.tcp() && !p.web()));
        assert!(d.machine("nope").is_none());
        let nets: Vec<(&str, &str, bool)> = d.networks.iter().map(|n| (n.name.as_str(), n.subnet.as_str(), n.internal)).collect();
        assert_eq!(nets, [("front", "10.40.0.0/24", false), ("back", "10.41.0.0/24", true)]);
        assert!(d.check_runners.iter().all(|r| r.starts_with("isoloom-check")) && !d.check_runners.is_empty(), "{:?}", d.check_runners);
    }

    #[test]
    fn a_vm_form_takes_the_specs_own_addresses_and_its_runners_from_the_vagrant_project() {
        let dir = lab();
        let containers = Described::of(&dir, Form::Containers).unwrap();
        let vms = Described::of(&dir, Form::Vms).unwrap();
        let ip = |d: &Described| d.machine("db").unwrap().interfaces.iter().map(|i| i.ip.clone()).collect::<Vec<_>>();
        assert_eq!(ip(&vms), ["10.41.0.20"], "the spec's own address");
        assert_eq!(ip(&containers).len(), 1, "one address on its one network");
        assert_eq!(vms.check_runners, [] as [String; 0], "no Vagrant project generated yet");
        let checks = lab::vagrant_dir(&dir, crate::runtime::Runtime::Vm).join("checks");
        std::fs::create_dir_all(&checks).unwrap();
        for f in ["web.sh", "isoloom-controller.sh", "notes.txt"] {
            std::fs::write(checks.join(f), "").unwrap();
        }
        assert_eq!(Described::of(&dir, Form::Vms).unwrap().check_runners, ["isoloom-controller", "web"]);
    }

    #[test]
    fn a_check_runners_lines_read_as_passed_or_failed_with_the_reason() {
        assert_eq!(check_line("web | isoloom-check: PASS http://web/ \r"), Some(("http://web/".into(), true, String::new())));
        assert_eq!(check_line("isoloom-check: FAIL db: port 5432 closed\n"), Some(("db".into(), false, "port 5432 closed".into())));
        assert_eq!(check_line("isoloom-check: FAIL bare"), Some(("bare".into(), false, String::new())));
        for other in ["isoloom-check: END 1 passed, 0 failed", "PASS without the marker", ""] {
            assert_eq!(check_line(other), None, "{other}");
        }
    }

    #[test]
    fn the_message_is_filled_for_its_target_and_a_lab_without_one_says_none() {
        let dir = lab();
        let said = message(&dir, lab::Target::Docker).expect("a message");
        assert_eq!(said.trim_end(), "Open http://localhost:8080/");
        let bare = crate::runtime::test_support::write_lab("version: 1\nname: t\nmachines:\n  web: { app: { image: nginx:1.27 } }\n");
        assert_eq!(message(&bare, lab::Target::Docker), None);
    }
}
