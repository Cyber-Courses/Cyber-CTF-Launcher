//! A run whose surroundings changed under it: the engine it runs on stopped while a step ran
//! (tests/resilience.md, T1).

use super::{Error, Kind, LabRun, errors};

impl LabRun {
    /// `e`, an operation's failure on this run, as the player reads it: on this machine's Docker,
    /// the engine stopped while a step ran is said as such ([`errors::engine_stopped`]).
    pub(super) fn said(&self, e: Error) -> Error {
        if self.kind == Kind::Docker && self.esxi.is_none() { errors::engine_stopped(e) } else { e }
    }
}
