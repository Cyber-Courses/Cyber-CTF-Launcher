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
    /// Where it is reachable on 127.0.0.1 (0: not published).
    pub published: u16,
    /// The port inside the container.
    pub target: u16,
}

/// One machine of a lab (a container or a VM), as the lab page and its diagram show it.
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabMachine")]
pub struct Machine {
    /// The container (a small computer on the lab network).
    pub name: String,
    /// Its state as the runtime reports it, e.g. `running`, `exited` or `unhealthy`.
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
    /// Its graphical console (its screen), as the lab's run offers it where it runs: `vnc` or
    /// `rdp`, or `none` and why. `None` when the run says nothing of it (a container).
    #[serde(default)]
    pub console: Option<Console>,
}

/// A machine's graphical console as the lab's run offers it.
#[derive(Serialize, Debug, PartialEq, Eq, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabConsole")]
pub struct Console {
    /// `vnc`, `rdp`, or `none`.
    pub protocol: String,
    /// Why there is none, or what it needs, in the run's words.
    pub note: Option<String>,
}

impl Console {
    /// The runner's offer of a machine's console, in the lab page's words.
    pub(crate) fn of(offer: &isothrum_runner::ConsoleOffer) -> Console {
        Console { protocol: super::isothrum::protocol_word(offer.protocol).to_owned(), note: offer.note.clone() }
    }
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
    /// Its address on that network, e.g. "172.20.0.4".
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
    /// In containers on this machine's Docker.
    Container,
    /// In VMs on this machine: a VM lab, or a container lab in a VM (one helper VM, or one VM per
    /// machine).
    LocalVm,
    /// On a saved server (Proxmox or ESXi).
    Server,
    /// In a cloud account.
    Cloud,
}

/// How a lab was parked: its machines kept on disk, to resume later without rebuilding them.
/// `Pause` saves the VMs' state (instant resume); `Shutdown` powers them off cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Park {
    /// The VMs' state saved to disk: they resume where they were.
    Pause,
    /// The VMs powered off cleanly, kept.
    Shutdown,
}

impl Park {
    /// Its id, as the parked marker and the webview spell it.
    pub fn id(self) -> &'static str {
        match self {
            Park::Pause => "pause",
            Park::Shutdown => "shutdown",
        }
    }

    /// The mode whose id is `id` (surrounding whitespace ignored), if there is one.
    pub fn from_id(id: &str) -> Option<Park> {
        match id.trim() {
            "pause" => Some(Park::Pause),
            "shutdown" => Some(Park::Shutdown),
            _ => None,
        }
    }
}

/// A lab as the UI shows it right now: whether it runs, its machines and networks, where it
/// runs and what can be done to it there.
#[derive(Serialize, Clone, Default, TS)]
#[serde(rename_all = "camelCase")]
pub struct LabStatus {
    /// Whether the lab is up (a lab whose launch failed reads as not running:
    /// [`LabStatus::unless_launch_failed`]).
    pub running: bool,
    /// Set when the launcher parked the lab (paused or shut down) and it isn't running: its
    /// machines exist and resume as they were, so the UI offers Resume, not "clean up".
    pub parked: Option<Park>,
    /// Its machines, the lab's plumbing among them ([`Machine::infra`]).
    pub machines: Vec<Machine>,
    /// The lab's network segments (Docker labs); empty when the runtime doesn't report them.
    pub networks: Vec<Network>,
    /// Loopback URL where the lab is reachable on this machine, once running (Docker labs
    /// with a published port). None for VM labs or when nothing is published yet.
    pub url: Option<String>,
    /// Name of the server host or cloud account the lab runs on; None when it runs on this
    /// machine (in a local VM too: `place` says that).
    pub host: Option<String>,
    /// The id of that saved server host or cloud account (its connection's), to find it among
    /// the saved ones; None when it runs on this machine.
    pub host_id: Option<String>,
    /// Unix time a cloud lab stops itself (auto-stop), if it does.
    pub expires_at: Option<u64>,
    /// Where it runs; set by the runtime dispatcher (None from the per-runtime probes).
    pub place: Option<Place>,
    /// The engine or hypervisor it runs on: "docker", or a Vagrant provider id ("virtualbox",
    /// "`vmware_desktop`", "parallels", ...), or a server/cloud provider. "On this machine" alone
    /// is misleading for a VM lab: this says which hypervisor to look in.
    pub provider: Option<String>,
    /// A container lab (catalogue Docker) running as one VM per machine — its VM recipes rather
    /// than its containers: on this machine (Vagrant), or on a cloud account (cloud-vm). Set by
    /// the status dispatcher. The UI names the form.
    pub per_machine: bool,
    /// What a cloud-services lab exposes (its module's outputs, by the spec's names): its
    /// entry points and credentials. Empty for the other labs.
    pub outputs: Vec<Output>,
    /// The lab's own words once it is up, placeholders filled in (cloud-services labs, whose
    /// message names their outputs); None for the others, whose message is in the deploy log.
    pub message: Option<String>,
    /// Why the auto-stop couldn't destroy this lab, while it is past its stop time (it may still
    /// be billing): set by the status dispatcher, None from the per-runtime probes.
    pub reap_error: Option<crate::error::Wire>,
    /// What can be done on the lab where it runs, in the form it was started in: set by the
    /// status dispatcher (`lifecycle::controls`), none from the per-runtime probes.
    pub controls: Vec<Control>,
}

/// One thing that can be done on a started lab where it runs: the backend's own rules, so the
/// lab page offers exactly what the lifecycle accepts.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabControl")]
pub enum Control {
    /// Pausing it (its machines' state saved).
    Pause,
    /// Shutting it down (its machines powered off, kept).
    Shutdown,
    /// Running its machines' provisioning again in place.
    Provision,
    /// Bringing machines that went down back up as they are, no rebuild (a resume of the run):
    /// the lab page's repair beside Reset.
    Repair,
}

impl LabStatus {
    /// The status of a lab whose last launch or resume `failed` (its worker's verdict): not
    /// running, even if its machines stayed up. A failed setup step leaves it unusable, and showing
    /// it running with a healthy diagram misleads; the lab page and the attack box's auto-start go
    /// by this alone, wherever the lab runs.
    #[must_use]
    pub fn unless_launch_failed(self, failed: bool) -> LabStatus {
        LabStatus { running: self.running && !failed, ..self }
    }
}

/// One thing a cloud-services lab exposes: a module output, under the spec's name for it.
#[derive(Serialize, Debug, Clone, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabOutput")]
pub struct Output {
    /// The spec's name for it.
    pub name: String,
    /// Its value, as the module gives it.
    pub value: String,
    /// Terraform marks it sensitive (a secret key): the UI masks it until asked.
    pub sensitive: bool,
}

#[cfg(test)]
mod tests {
    use super::{LabStatus, Park};

    #[test]
    fn a_failed_launch_reads_as_not_running_even_with_machines_up() {
        // Regression: only server and cloud labs used to be masked, so a local Docker,
        // docker-in-VM or VM lab read as running after a failed launch.
        let up = || LabStatus { running: true, ..LabStatus::default() };
        assert!(up().unless_launch_failed(false).running, "nothing failed: running");
        assert!(!up().unless_launch_failed(true).running);
        assert!(!LabStatus::default().unless_launch_failed(false).running);
    }

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
