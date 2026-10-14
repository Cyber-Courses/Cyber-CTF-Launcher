//! The lab status model: what a running lab looks like to the UI. Shared by every runtime
//! (Docker, VM, Terraform), kept apart from the command surface so adding a field here
//! doesn't touch command registration or dispatch.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A port the software inside a container binds: `target` is the port inside the
/// container, `published` is where it is reachable on 127.0.0.1 (0 = not published).
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabPort")]
pub struct Port {
    pub published: u16,
    pub target: u16,
}

#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabMachine")]
pub struct Machine {
    /// The container (a small computer on the lab network).
    pub name: String,
    pub state: String,
    /// The image = the software running inside the container, e.g. "mysql:8.0".
    #[serde(default)]
    pub image: String,
    /// The container's address on the lab network, e.g. "172.20.0.4" (empty if unknown).
    #[serde(default)]
    pub ip: String,
    /// Ports the software inside it binds (for the network diagram).
    #[serde(default)]
    pub ports: Vec<Port>,
    /// Every network the machine is plugged into, with its address there. A machine on
    /// two networks is a pivot (dual-homed); empty when the runtime can't tell.
    #[serde(default)]
    pub interfaces: Vec<Interface>,
    /// The services running inside, as the lab declares them (compose labels
    /// `cyberctf.service.<name>: "<kind>:<port>,<port>"`). A container may run several;
    /// empty when the lab declares none (never inferred).
    #[serde(default)]
    pub services: Vec<Service>,
    /// Part of the lab's plumbing, not a target: the controller that provisions the machines
    /// (Isoloom's `isoloom-controller`). Powered off once the lab is up, hidden from the
    /// diagram, never counted as "down". (Isoloom will expose machine roles itself; until then
    /// the controller is known by its name.)
    #[serde(default)]
    pub infra: bool,
}

/// One service inside a machine, as declared by the lab.
#[derive(Serialize, Debug, PartialEq, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabService")]
pub struct Service {
    /// The lab's name for it, e.g. "portal", "ssh".
    pub name: String,
    /// What it is, as declared: "web", "database", "cache", "worker", "ssh" or free text.
    pub kind: String,
    /// The ports it listens on inside the container.
    pub ports: Vec<u16>,
}

/// One network interface of a machine: the lab network it sits on and its address there.
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabInterface")]
pub struct Interface {
    /// The lab's own name for the network (the compose key, e.g. "dmz"), not Docker's.
    pub network: String,
    pub ip: String,
}

/// A network segment of the lab (a Docker network = a switch the machines plug into).
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabNetwork")]
pub struct Network {
    /// The lab's own name for it (the compose key), e.g. "default", "dmz", "internal".
    pub name: String,
    /// CIDR, e.g. "172.20.0.0/16" (empty if Docker didn't report one).
    pub subnet: String,
    /// No route out (compose `internal: true`): its machines can't reach the internet.
    pub internal: bool,
}

/// Where a lab runs, for the UI: its own runtime here, a VM here, a server, or a cloud account.
#[derive(Serialize, Clone, Copy, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "LabPlace")]
pub enum Place {
    Container,
    LocalVm,
    Server,
    Cloud,
}

/// How a lab was parked: its machines kept on disk, to resume later without rebuilding them.
/// `Pause` saves the VMs' state (instant resume); `Shutdown` powers them off cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Park {
    Pause,
    Shutdown,
}

impl Park {
    pub fn id(self) -> &'static str {
        match self {
            Park::Pause => "pause",
            Park::Shutdown => "shutdown",
        }
    }

    pub fn from_id(id: &str) -> Option<Park> {
        match id.trim() {
            "pause" => Some(Park::Pause),
            "shutdown" => Some(Park::Shutdown),
            _ => None,
        }
    }
}

#[derive(Serialize, Clone, Default, TS)]
#[serde(rename_all = "camelCase")]
pub struct LabStatus {
    pub running: bool,
    /// Set when the launcher parked the lab (paused or shut down) and it isn't running: its
    /// machines exist and resume as they were, so the UI offers Resume, not "clean up".
    pub parked: Option<Park>,
    pub machines: Vec<Machine>,
    /// The lab's network segments (Docker labs); empty when the runtime doesn't report them.
    pub networks: Vec<Network>,
    /// Loopback URL where the lab is reachable on this machine, once running (Docker labs
    /// with a published port). None for VM labs or when nothing is published yet.
    pub url: Option<String>,
    /// Name of the server host a VM lab runs on; None when it runs on this machine.
    pub host: Option<String>,
    /// Unix time a cloud lab stops itself (auto-stop), if it does.
    pub expires_at: Option<u64>,
    /// Where it runs; set by the runtime dispatcher (None from the per-runtime probes).
    pub place: Option<Place>,
    /// The engine or hypervisor it runs on: "docker", or a Vagrant provider id ("virtualbox",
    /// "vmware_desktop", "parallels", ...), or a server/cloud provider. "On this machine" alone
    /// is misleading for a VM lab: this says which hypervisor to look in.
    pub provider: Option<String>,
    /// Where the attack box sits when the lab host runs it (server and cloud labs): the local
    /// attack box probe can't see a container on another machine.
    pub attacker: Option<AttackerAt>,
    /// What a cloud-services lab exposes (its module's outputs, by the spec's names): its
    /// entry points and credentials. Empty for the other labs.
    pub outputs: Vec<Output>,
    /// The lab's own words once it is up, placeholders filled in (cloud-services labs, whose
    /// message names their outputs); None for the others, whose message is in the deploy log.
    pub message: Option<String>,
    /// Why the auto-stop couldn't destroy this lab, while it is past its stop time (it may still
    /// be billing): set by the status dispatcher, None from the per-runtime probes.
    pub reap_error: Option<String>,
}

/// One thing a cloud-services lab exposes: a module output, under the spec's name for it.
#[derive(Serialize, Debug, Clone, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabOutput")]
pub struct Output {
    pub name: String,
    pub value: String,
    /// Terraform marks it sensitive (a secret key): the UI masks it until asked.
    pub sensitive: bool,
}

/// The attack box's address on the lab network.
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabAttacker")]
pub struct AttackerAt {
    pub ip: String,
    pub lab_network: String,
}

#[cfg(test)]
mod tests {
    use super::Park;

    #[test]
    fn park_ids_round_trip_and_read_with_surrounding_whitespace_only() {
        for p in [Park::Pause, Park::Shutdown] {
            assert_eq!(Park::from_id(p.id()), Some(p));
            assert_eq!(Park::from_id(&format!(" {}\r\n", p.id())), Some(p), "a marker file's trailing newline");
        }
        // Case and extra words are not tolerated: a marker reads as "not parked" rather than guessed.
        for bad in ["Pause", "SHUTDOWN", "pause now", "", "halt", "suspend"] {
            assert_eq!(Park::from_id(bad), None, "{bad:?}");
        }
    }
}
