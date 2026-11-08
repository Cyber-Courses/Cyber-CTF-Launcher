//! The learner's attack box as the runner sees it: the lab's on-demand access machine, or the
//! box the runner stands up itself (a container beside a container lab's, a VM added to a VM
//! lab's running environment, `isoloom-access` either way). The launcher never runs it — it asks
//! the runner to hot-add it onto a chosen lab network (with the box image the learner picked, else
//! the lab's default), to take it out again, to report it, and to build the shell command into
//! it. Status, networks and the shell come from the run, so the lab page treats the box like any
//! other machine.

use isoloom_manifest::Role;
use isothrum_runner::{MachineState, MachineStatus, ShellCommand, Status};
use serde::Serialize;

/// The attack box beside a lab, as the lab page shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AttackStatus {
    /// Whether the box is up (the runner has it running).
    pub running: bool,
    /// Its address on the network it joined (what the targets see), once running.
    pub ip: String,
    /// The lab network it is on (the lab's name for it), once running.
    pub lab_network: String,
    /// Every lab network the box can join, for the learner to pick from (the only one is used
    /// without asking). Known before the box is up, so the page can offer the choice.
    pub networks: Vec<String>,
    /// The command line that opens a shell in the box (shown so the player can also run it).
    /// Empty when the runner makes no box for the run: a VM lab on a server or in a cloud
    /// account (a container lab there gets one, in the VM its containers run in).
    pub shell_cmd: String,
    /// Whether the box is a VM here: beside a lab running as VMs on this machine (the lab's own
    /// attack VM, or the one the runner adds to the running lab).
    pub vm: bool,
    /// Whether the box boots from the Vagrant box picked in Settings: a VM lab's own attack VM.
    /// Otherwise it runs the attack box image: beside a container lab's containers, inside the VM
    /// of a container lab run in one, or inside the attack VM the runner adds to a VM lab.
    pub picked_box: bool,
}

/// The define name of the attack VM the runner adds to a VM lab itself (Isoloom's reserved
/// namespace: a lab's own machine is never called so).
const ADHOC_NAME: &str = "isoloom-access";

/// The run's on-demand access machine, if it has one.
fn access(status: &Status) -> Option<&MachineStatus> {
    status.machines.iter().find(|m| m.role == Role::Access)
}

/// The networks the box can join: the lab's networks, in the run's order.
fn networks(status: &Status) -> Vec<String> {
    status.networks.iter().map(|n| n.name.clone()).collect()
}

/// A shell command as one line the player can paste in their terminal: the variables it needs
/// first (`VAR=value`, as a shell takes them), then the program and its arguments, each quoted
/// where it must be.
pub(super) fn display(cmd: &ShellCommand) -> String {
    let words: Vec<String> =
        cmd.env.iter().map(|(k, v)| format!("{k}={v}")).chain(std::iter::once(cmd.program.clone())).chain(cmd.args.iter().cloned()).collect();
    shell_words::join(&words)
}

/// Whether the lab declares an attack VM of its own: an access machine that isn't the one the
/// runner adds (which only appears once added).
fn own_access(status: &Status) -> bool {
    status.machines.iter().any(|m| m.role == Role::Access && m.name != ADHOC_NAME)
}

/// The attack box's status from the run's status: up when the access machine runs, its address on
/// `network` (or its first), the lab's networks, the `shell_cmd` the runner built, and what the
/// box is here (`vm`: the lab runs as VMs on this machine; the picked Vagrant box boots it only
/// when the lab declares its own attack VM).
pub(super) fn status_of(status: &Status, network: Option<&str>, shell_cmd: String, vm: bool) -> AttackStatus {
    let machine = access(status);
    let running = machine.is_some_and(|m| matches!(m.state, MachineState::Running | MachineState::Unhealthy));
    let addresses = machine.map(|m| &m.addresses);
    let on = network
        .filter(|n| addresses.is_some_and(|a| a.contains_key(*n)))
        .map(str::to_owned)
        .or_else(|| addresses.and_then(|a| a.keys().next().cloned()))
        .unwrap_or_default();
    let ip = addresses.and_then(|a| a.get(&on)).cloned().unwrap_or_default();
    AttackStatus {
        running,
        ip: if running { ip } else { String::new() },
        lab_network: on,
        networks: networks(status),
        shell_cmd,
        vm,
        picked_box: vm && own_access(status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A VM lab's status as the runner reports it: its web VM, and the attack VM the runner added
    /// in `state` at its address on the lab network (absent: none added yet).
    fn vm_lab(state: &str) -> Status {
        serde_json::from_value(serde_json::json!({
            "state": "ready",
            "provider": "qemu",
            "machines": [
                { "name": "web", "role": "target", "state": "running", "addresses": { "lab": "10.61.0.10" }, "published": [] },
                { "name": "isoloom-access", "role": "access", "state": state, "addresses": { "lab": "10.61.0.252" }, "published": [] }
            ],
            "networks": [{ "name": "lab", "cidr": "10.61.0.0/24", "internet": false, "native": "isoloom-t-lab" }],
            "outputs": [],
            "message": null
        }))
        .expect("a run status")
    }

    #[test]
    fn a_vm_labs_attack_vm_is_the_runs_access_machine_down_until_hot_added() {
        let shell = "VAGRANT_DOTFILE_PATH=/runs/t/vagrant vagrant ssh isoloom-access".to_owned();
        let down = status_of(&vm_lab("absent"), None, shell.clone(), true);
        assert_eq!((down.running, down.ip.as_str(), down.networks.as_slice()), (false, "", ["lab".to_owned()].as_slice()));
        assert_eq!(down.shell_cmd, shell, "the shell command is known before the box is up");
        // The runner's own VM runs the attack box image: no Vagrant box to pick.
        assert!(down.vm && !down.picked_box, "{down:?}");
        let up = status_of(&vm_lab("running"), None, shell, true);
        assert_eq!((up.running, up.ip.as_str(), up.lab_network.as_str()), (true, "10.61.0.252", "lab"));
    }

    #[test]
    fn a_labs_own_attack_vm_boots_from_the_picked_box_and_a_container_box_is_no_vm() {
        let mut own = vm_lab("absent");
        own.machines[1].name = "kali".into();
        let status = status_of(&own, None, String::new(), true);
        assert!(status.vm && status.picked_box, "{status:?}");
        let container = status_of(&vm_lab("running"), None, String::new(), false);
        assert!(!container.vm && !container.picked_box, "{container:?}");
    }

    #[test]
    fn the_shell_line_is_pasteable_with_the_variables_it_needs() {
        let plain = ShellCommand { program: "docker".into(), args: ["compose", "exec", "kali", "bash"].map(String::from).to_vec(), cwd: None, env: Vec::new() };
        assert_eq!(display(&plain), "docker compose exec kali bash");
        let in_vm = ShellCommand {
            program: "vagrant".into(),
            args: ["ssh", "default", "-c", "sudo docker exec -it 'x-isoloom-access' sh"].map(String::from).to_vec(),
            cwd: None,
            env: vec![("VAGRANT_DOTFILE_PATH".into(), "/runs/x y/vagrant".into())],
        };
        let line = display(&in_vm);
        assert!(line.starts_with("'VAGRANT_DOTFILE_PATH=/runs/x y/vagrant' vagrant ssh default -c "), "{line}");
        assert_eq!(shell_words::split(&line).unwrap().last().map(String::as_str), Some("sudo docker exec -it 'x-isoloom-access' sh"), "{line}");
    }
}
