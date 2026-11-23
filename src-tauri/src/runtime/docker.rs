//! What the launcher reads of container labs from Docker itself; the runner starts, parks and
//! stops them (`isothrum`). Split by concern:
//! - `compose`: a lab's Compose project name, and an older version's attack-box names
//! - `check`: the exploitability check's result, as the lab page shows it
//! - `engine`: the Docker context behind an engine, and the daemon's answers as the launcher's errors

mod check;
mod compose;
mod engine;

pub use check::{Check, CheckResult};
pub use compose::{lab_of_project, project};
pub(crate) use engine::{context_error, engine_context, engine_of, no_answer};
