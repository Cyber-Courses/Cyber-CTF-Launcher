//! What the launcher reads of container labs from Docker itself; the runner starts, parks and
//! stops them (`isothrum`). Split by concern:
//! - `compose`: talking to the compose CLI and parsing its output (one place for the flags)
//! - `lifecycle`: the launch's ports setting, and Docker's address-pool clash said as what to do
//! - `inspect`: whether a lab's project is up here, the reachable URL, the networks' blocks
//! - `check`: the exploitability check's result, as the lab page shows it

mod check;
mod compose;
mod inspect;
mod lifecycle;

pub use check::{Check, CheckResult};
pub use compose::{LEGACY_ATTACK_BOX_SUFFIXES, legacy_attack_box_names, project};
pub use inspect::{primary_url, status, subnets_in_use};
pub use lifecycle::PORTS_ENV;
pub(crate) use lifecycle::explain_pool_overlap;
