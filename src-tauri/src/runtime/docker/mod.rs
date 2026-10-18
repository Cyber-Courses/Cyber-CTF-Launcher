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

pub use check::{Check, CheckResult};
pub use compose::project;
pub use inspect::{primary_url, status, subnets_in_use};
pub(crate) use lifecycle::explain_pool_overlap;
pub use lifecycle::{PORTS_ENV, stop};
