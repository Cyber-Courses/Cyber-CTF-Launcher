//! What the launcher reads of container labs from Docker itself; the runner starts, parks and
//! stops them (`isothrum`). Split by concern:
//! - `compose`: talking to the compose CLI and parsing its output (one place for the flags)
//! - `lifecycle`: the launch's ports setting, and Docker's address-pool clash said as what to do
//! - `inspect`: a running lab's reachable URL, the address blocks Docker's networks hold
//! - `check`: the exploitability check's result, as the lab page shows it

mod check;
mod compose;
mod inspect;
mod launch_env;

pub use check::{Check, CheckResult};
pub use compose::{LEGACY_ATTACK_BOX_SUFFIXES, lab_of_project, legacy_attack_box_names, project};
pub use inspect::{primary_url, subnets_in_use};
pub use launch_env::PORTS_ENV;
pub(crate) use launch_env::explain_pool_overlap;
