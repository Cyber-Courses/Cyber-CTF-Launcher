//! How the player reaches a lab from this computer: its machines (a shell, its checks) and its
//! published ports (its link). One strategy, chosen once per lab location, that every way in
//! asks rather than deciding it again:
//!
//! - [`Reach::Direct`]: the lab's addresses are routed to this computer. A local lab, a server
//!   whose lab bridge is on its network, a cloud's public addresses.
//! - [`Reach::ThroughNode`]: only a node of the server reaches them (a Proxmox node whose lab
//!   bridge is internal to it: `lab::NODE_KEY`'s node, `server::proxmox_jump_for_host`). Every way
//!   in goes through it first, signed in with the runner's key, its host key the one the player
//!   confirmed: a shell's SSH, the runner's own SSH for the checks (its `ssh_jump` value), and a
//!   local forward to each published port.
//!
//! The choice is made where the lab's place is: from the server's profile (its kind, whether its
//! lab bridge is internal), when an operation applies the module ([`Reach::of_place`]). The run
//! records it among its values (`ssh_jump`), so what only reads the run (its status, a shell, its
//! checks) finds it there ([`Reach::of_record`]) without asking the server again.

use isothrum_runner::RunRecord;

use super::server::ServerPlace;

/// The run's value naming the node every SSH of the runner goes through first (Isothrum's).
pub(crate) const JUMP_VALUE: &str = "ssh_jump";

/// How the player reaches a lab from this computer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Reach {
    /// Its addresses are routed here.
    #[default]
    Direct,
    /// Only the node `login` (`root@<node>`) reaches them.
    ThroughNode {
        /// The node's login, as a jump names it.
        login: String,
    },
}

impl Reach {
    /// The reach of a lab on `place`: through its node when its lab bridge is internal to it.
    pub(crate) fn of_place(place: &ServerPlace) -> Reach {
        place.jump.as_deref().map_or(Reach::Direct, Reach::through)
    }

    /// The reach of the run `record` keeps, as its last start or update recorded it; direct for
    /// one that recorded none (a cloud's, a server's on a bridge this computer reaches, one
    /// started before the reach was recorded).
    pub(crate) fn of_record(record: Option<&RunRecord>) -> Reach {
        record.and_then(|r| r.env.get(JUMP_VALUE)).filter(|j| !j.is_empty()).map_or(Reach::Direct, |j| Reach::through(j))
    }

    /// Through the node `login`.
    fn through(login: &str) -> Reach {
        Reach::ThroughNode { login: login.to_owned() }
    }

    /// The node every way in goes through first, if any.
    pub(crate) fn node(&self) -> Option<&str> {
        match self {
            Reach::Direct => None,
            Reach::ThroughNode { login } => Some(login),
        }
    }

    /// The run's values that say it: the runner's SSH (a console, a command) goes through the node
    /// first, and the run records it for what only reads it.
    pub(crate) fn values(&self) -> Vec<(String, String)> {
        self.node().map(|login| (JUMP_VALUE.to_owned(), login.to_owned())).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(env: &[(&str, &str)]) -> RunRecord {
        serde_json::from_value(serde_json::json!({
            "version": 0, "name": "cyberctf-web", "project": "/labs/web", "format": "terraform", "state": "ready",
            "env": env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect::<std::collections::BTreeMap<_, _>>(),
        }))
        .unwrap()
    }

    #[test]
    fn a_lab_on_a_bridge_internal_to_its_node_is_reached_through_it_and_the_run_keeps_it() {
        let place = ServerPlace { jump: Some("root@node.lab".into()), ..ServerPlace::default() };
        let reach = Reach::of_place(&place);
        assert_eq!(reach.node(), Some("root@node.lab"));
        assert_eq!(reach.values(), [("ssh_jump".to_owned(), "root@node.lab".to_owned())]);
        // What the run recorded of it reads back the same.
        assert_eq!(Reach::of_record(Some(&record(&[("ssh_jump", "root@node.lab"), ("node", "pve")]))), reach);
    }

    #[test]
    fn a_lab_this_computer_reaches_is_reached_directly() {
        assert_eq!(Reach::of_place(&ServerPlace::default()), Reach::Direct);
        assert!(Reach::Direct.values().is_empty(), "nothing for the runner to go through");
        assert_eq!(Reach::of_record(Some(&record(&[("node", "pve")]))), Reach::Direct);
        assert_eq!(Reach::of_record(Some(&record(&[("ssh_jump", "")]))), Reach::Direct);
        assert_eq!(Reach::of_record(None), Reach::Direct, "no run recorded");
    }
}
