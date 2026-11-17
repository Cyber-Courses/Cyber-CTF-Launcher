//! What the launcher reads of container labs from Docker itself; the runner starts, parks and
//! stops them (`isothrum`). Split by concern:
//! - `compose`: a lab's Compose project name, and an older version's attack-box names
//! - `inspect`: the address blocks Docker's networks hold, and whether Docker answers
//! - `check`: the exploitability check's result, as the lab page shows it

mod check;
mod compose;
mod inspect;

pub use check::{Check, CheckResult};
pub use compose::{lab_of_project, project};
pub use inspect::answers;
