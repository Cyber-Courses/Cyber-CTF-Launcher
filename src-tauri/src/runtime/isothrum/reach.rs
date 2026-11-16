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

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use isothrum_runner::RunRecord;

use super::server::ServerPlace;
use super::tunnel::{Forward, LOOPBACK};

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

/// The forwards open in this process, by run, each by the `host:port` it goes to: a lab's links
/// while it runs. Ended with the lab ([`Reach::close`]), or by the app's exit with every tool it
/// runs.
static FORWARDS: LazyLock<tokio::sync::Mutex<BTreeMap<String, BTreeMap<String, Forward>>>> = LazyLock::new(Default::default);

/// The first port tried for a forward of run `run` to `to`: the same one every time (its link
/// stays the same after the app restarts), out of the ports systems hand out on their own; a free
/// one when it is taken.
fn preferred_port(run: &str, to: &str) -> u16 {
    // FNV-1a: stable across builds and platforms, unlike the standard hasher.
    let hash = format!("{run}\0{to}").bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    20_000 + u16::try_from(hash % 10_000).unwrap_or_default()
}

impl Reach {
    /// The module's `published` entries of run `run` (`"<machine>/<port>"` to `"<host>:<port>"`),
    /// where this computer reaches each one: as they are when the lab is reached directly; through
    /// the node, at the local end of a forward to each, opened with `identity` (the runner's key)
    /// when it isn't open yet or its ssh ended. A forward to what the lab no longer publishes ends.
    /// An entry whose forward doesn't open is left out (no link rather than a broken one), saying
    /// why in the log; one without its address yet stays as it is.
    pub(crate) async fn published(&self, run: &str, identity: &Path, published: BTreeMap<String, String>) -> BTreeMap<String, String> {
        let Reach::ThroughNode { login } = self else { return published };
        let mut open = FORWARDS.lock().await;
        let forwards = open.entry(run.to_owned()).or_default();
        forwards.retain(|to, _| published.values().any(|v| v == to));
        let mut reached = BTreeMap::new();
        for (entry, to) in published {
            let Some((host, port)) = to.rsplit_once(':').and_then(|(h, p)| Some((h, p.parse::<u16>().ok()?))).filter(|(h, _)| !h.is_empty()) else {
                reached.insert(entry, to);
                continue;
            };
            let open_now = match forwards.get_mut(&to) {
                Some(forward) => forward.runs().await,
                None => false,
            };
            if !open_now {
                // The port it had before (its ssh ended), else the run's own for it.
                let before = forwards.remove(&to).map_or_else(|| preferred_port(run, &to), |f| f.port);
                match Forward::open(login, identity, (host, port), Some(before)).await {
                    Ok(forward) => {
                        forwards.insert(to.clone(), forward);
                    }
                    Err(e) => {
                        log::warn!("the lab's {entry} isn't reachable from this computer: {e}");
                        continue;
                    }
                }
            }
            if let Some(forward) = forwards.get(&to) {
                reached.insert(entry, format!("{LOOPBACK}:{}", forward.port));
            }
        }
        reached
    }

    /// Ends run `run`'s forwards: its lab stopped.
    pub(crate) async fn close(run: &str) {
        FORWARDS.lock().await.remove(run);
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
