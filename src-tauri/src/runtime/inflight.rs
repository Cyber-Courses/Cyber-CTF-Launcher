//! The names the UI gives a lab's operations ([`Operation`]). What runs on which lab right now,
//! and one operation per lab at a time, are the run service's to say (`run_service`), the runner
//! service refusing a second operation on a lab's run: the launcher keeps no lock of its own.

/// A lab operation as the UI names it, a pause and a shut down told apart.
#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq, Debug, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    /// A launch.
    Launch,
    /// A resume of a parked lab.
    Resume,
    /// A pause.
    Pause,
    /// A shut down.
    Shutdown,
    /// A provisioning run.
    Provision,
    /// A stop.
    Stop,
    /// The attack box being added.
    Attach,
    /// The attack box being taken out.
    Detach,
}
