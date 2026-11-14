//! The learner's attack box as the lab page shows it: the machine the launcher adds to the lab,
//! read by its name from the run's status like any other machine: whether it runs and its address
//! on the lab network; beside it, the networks it can join.

use isothrum_runner::{MachineState, Status};
use serde::Serialize;

/// The attack box beside a lab, as the lab page shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AttackStatus {
    /// Whether the launcher reaches a run of the lab to put a box in (`attack_box::locate`):
    /// not on an ESXi host, nor beside a cloud-services lab.
    pub reachable: bool,
    /// Whether the box is up (the runner has it running).
    pub running: bool,
    /// Its address on the network it joined (what the targets see), once running.
    pub ip: String,
    /// The lab network it is on (the lab's name for it), once running.
    pub lab_network: String,
    /// The lab networks the learner may pick for the box (the only one is used without asking):
    /// none when the lab declares where the player sits, or once the box is in. Known before the
    /// box is up, so the page can offer the choice.
    pub networks: Vec<String>,
    /// The network the box joins when the learner picks none (`attack_box::Networks`).
    pub default_network: String,
    /// Whether the box is a VM: a Kali VM among a lab's VMs, wherever the lab runs.
    pub vm: bool,
    /// Its machine's name in the lab's run (what its screen window is opened by).
    pub machine: String,
    /// Its screen (graphical console), as the run offers it; none for a container box.
    pub console: Option<crate::runtime::Console>,
}

/// The box `name` from the run's status: up when it runs, and whether it is a VM (`vm`). The
/// runner reports no machine's address and no screen: the box shows none of either until it
/// does.
pub(super) fn status_of(status: &Status, name: &str, vm: bool) -> AttackStatus {
    let machine = status.machines.iter().find(|m| m.name == name);
    let running = machine.is_some_and(|m| matches!(m.state, MachineState::Running | MachineState::Unhealthy));
    AttackStatus { running, vm, machine: name.to_owned(), console: None, ..AttackStatus::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A VM lab's status as the runner reports it: its web VM, and the box `kali` in `state`
    /// (absent: not added yet).
    fn vm_lab(state: &str) -> Status {
        serde_json::from_value(serde_json::json!({
            "state": "ready",
            "provider": "qemu",
            "machines": [
                { "name": "web", "state": "running", "published": [] },
                { "name": "kali", "state": state, "published": [] }
            ],
            "outputs": []
        }))
        .expect("a run status")
    }

    #[test]
    fn the_box_is_found_by_its_name_down_until_it_runs() {
        let down = status_of(&vm_lab("absent"), "kali", true);
        assert_eq!((down.running, down.ip.as_str(), down.lab_network.as_str()), (false, "", ""), "no address nor network until it runs");
        let up = status_of(&vm_lab("running"), "kali", true);
        assert_eq!((up.running, up.ip.as_str()), (true, ""), "the runner reports no address");
        assert!(!status_of(&vm_lab("running"), "nobody", false).running, "no such machine: no box");
    }

    /// Regression: the box's start, in the deploy worker, held `attackbox` in Vagrant, and the
    /// page's status poll failed with Vagrant's lock refusal. The runner now says the machine is
    /// busy: the box isn't up yet, and nothing of it is an error.
    #[test]
    fn a_box_another_process_is_setting_up_is_not_up_yet() {
        let busy = status_of(&vm_lab("busy"), "kali", true);
        assert_eq!((busy.running, busy.ip.as_str()), (false, ""));
        assert!(busy.console.is_none(), "no screen is offered yet");
    }

    #[test]
    fn the_box_carries_its_machine() {
        let up = status_of(&vm_lab("running"), "kali", true);
        assert_eq!(up.machine, "kali");
        assert_eq!(up.console, None, "no screen is offered yet");
    }
}
