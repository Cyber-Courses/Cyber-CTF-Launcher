//! What the launcher reads of container labs from Docker itself; the runner starts, parks and
//! stops them (`isothrum`). Split by concern:
//! - `compose`: a lab's Compose project name, and an older version's attack-box names
//! - `check`: the exploitability check's result, as the lab page shows it

mod check;
mod compose;

pub use check::{Check, CheckResult};
pub use compose::{lab_of_project, project};
