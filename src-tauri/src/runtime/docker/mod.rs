//! Docker-Compose lab engine, split by concern:
//! - `compose`: talking to the compose CLI and parsing its output (one place for the flags)
//! - `lifecycle`: start / stop, with the host-port collision check
//! - `inspect`: status, networks, interfaces, the reachable URL
//! - `check`: the exploitability self-check
//!
//! A new signal (a health field, another network fact) touches one of these, not all of
//! them, so parallel work stops colliding in a single god-function.

mod check;
mod compose;
mod inspect;
mod lifecycle;

pub use check::{Check, check};
pub use compose::{attack_network, project};
pub use inspect::{HostProbe, containers, host_probe_script, lab_network_names, primary_url, short_network, status, status_from_host, subnets_in_use};
pub use lifecycle::{PORTS_ENV, park, published, resume, start, stop};
