//! The learner's attack box as the runner sees it: the machine the launcher adds to the lab
//! (`attack_machine`), read by its name from the run's status like any other machine: whether it
//! runs, its address on the lab network, the lab's networks, and the console into it.

use isothrum_runner::{MachineState, ShellCommand, Status};
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
    /// Empty when the run has no way into the box (a lab with no attack machine of its own,
    /// before the launcher adds one).
    pub shell_cmd: String,
    /// Whether the box is a VM here: beside a lab running as VMs on this machine.
    pub vm: bool,
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

/// The box `name` from the run's status: up when it runs, its address on `network` (or its
/// first), the lab's networks, the `shell_cmd` the runner built, and whether it is a VM (`vm`).
pub(super) fn status_of(status: &Status, name: &str, network: Option<&str>, shell_cmd: String, vm: bool) -> AttackStatus {
    let machine = status.machines.iter().find(|m| m.name == name);
    let running = machine.is_some_and(|m| matches!(m.state, MachineState::Running | MachineState::Unhealthy));
    let addresses = machine.map(|m| &m.addresses);
    let on = network
        .filter(|n| addresses.is_some_and(|a| a.contains_key(*n)))
        .map(str::to_owned)
        .or_else(|| addresses.and_then(|a| a.keys().next().cloned()))
        .unwrap_or_default();
    let ip = addresses.and_then(|a| a.get(&on)).cloned().unwrap_or_default();
    AttackStatus { running, ip: if running { ip } else { String::new() }, lab_network: on, networks: networks(status), shell_cmd, vm }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A VM lab's status as the runner reports it: its web VM, and the box `kali` in `state` at
    /// its address on the lab network (absent: not added yet).
    fn vm_lab(state: &str) -> Status {
        serde_json::from_value(serde_json::json!({
            "state": "ready",
            "provider": "qemu",
            "machines": [
                { "name": "web", "role": "target", "state": "running", "addresses": { "lab": "10.61.0.10" }, "published": [] },
                { "name": "kali", "role": "access", "state": state, "addresses": { "lab": "10.61.0.252" }, "published": [] }
            ],
            "networks": [{ "name": "lab", "cidr": "10.61.0.0/24", "internet": false, "native": "isoloom-t-lab" }],
            "outputs": [],
            "message": null
        }))
        .expect("a run status")
    }

    #[test]
    fn the_box_is_found_by_its_name_down_until_it_runs() {
        let shell = "VAGRANT_DOTFILE_PATH=/runs/t/vagrant vagrant ssh kali".to_owned();
        let down = status_of(&vm_lab("absent"), "kali", None, shell.clone(), true);
        assert_eq!((down.running, down.ip.as_str(), down.networks.as_slice()), (false, "", ["lab".to_owned()].as_slice()));
        assert_eq!(down.shell_cmd, shell, "the shell command is known before the box is up");
        let up = status_of(&vm_lab("running"), "kali", None, shell, true);
        assert_eq!((up.running, up.ip.as_str(), up.lab_network.as_str()), (true, "10.61.0.252", "lab"));
        assert!(!status_of(&vm_lab("running"), "nobody", None, String::new(), false).running, "no such machine: no box");
    }

    #[test]
    fn the_shell_line_is_pasteable_with_the_variables_it_needs() {
        let plain = ShellCommand { program: "docker".into(), args: ["compose", "exec", "kali", "bash"].map(String::from).to_vec(), cwd: None, env: Vec::new() };
        assert_eq!(display(&plain), "docker compose exec kali bash");
        let in_vm = ShellCommand {
            program: "vagrant".into(),
            args: ["ssh", "default", "-c", "sudo docker exec -it 'x-web' sh"].map(String::from).to_vec(),
            cwd: None,
            env: vec![("VAGRANT_DOTFILE_PATH".into(), "/runs/x y/vagrant".into())],
        };
        let line = display(&in_vm);
        assert!(line.starts_with("'VAGRANT_DOTFILE_PATH=/runs/x y/vagrant' vagrant ssh default -c "), "{line}");
        assert_eq!(shell_words::split(&line).unwrap().last().map(String::as_str), Some("sudo docker exec -it 'x-web' sh"), "{line}");
    }
}
