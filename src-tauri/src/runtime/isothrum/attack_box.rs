//! The learner's attack box as the runner sees it: the lab's on-demand access machine. The
//! launcher never runs it — it asks the runner to hot-add it onto a chosen lab network (with the
//! box image the learner picked), to take it out again, to report it, and to build the shell
//! command into it. Status, networks and the shell come from the run, so the lab page treats the
//! box like any other machine.

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
    pub shell_cmd: String,
}

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

/// The attack box's status from the run's status: up when the access machine runs, its address on
/// `network` (or its first), the lab's networks, and the `shell_cmd` the runner built.
pub(super) fn status_of(status: &Status, network: Option<&str>, shell_cmd: String) -> AttackStatus {
    let machine = access(status);
    let running = machine.is_some_and(|m| matches!(m.state, MachineState::Running | MachineState::Unhealthy));
    let addresses = machine.map(|m| &m.addresses);
    let on = network
        .filter(|n| addresses.is_some_and(|a| a.contains_key(*n)))
        .map(str::to_owned)
        .or_else(|| addresses.and_then(|a| a.keys().next().cloned()))
        .unwrap_or_default();
    let ip = addresses.and_then(|a| a.get(&on)).cloned().unwrap_or_default();
    AttackStatus { running, ip: if running { ip } else { String::new() }, lab_network: on, networks: networks(status), shell_cmd }
}

#[cfg(test)]
mod tests {
    use super::*;

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
