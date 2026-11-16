//! The runner's events as the launcher's progress: the lines of a deploy log (the lab page shows
//! them as they come, and the sidebar takes its step from the last one). A step's own line (its note), what its tool prints, and what the runner warns about are shown;
//! state moves, outputs and the final result or error are not (the caller says how it ended).

use super::super::deploy_log;
use isothrum_runner::{EventKind, StepState, WarningCode};

/// The deploy log's line for an event, if it gets one.
pub(super) fn line(kind: &EventKind) -> Option<String> {
    match kind {
        EventKind::Step { state: StepState::Running, note: Some(note), .. } => Some(note.clone()),
        EventKind::Log { line, .. } => Some(line.clone()),
        EventKind::Warning { code, message } => warning(*code, message),
        _ => None,
    }
}

/// What the player reads for one of the runner's warnings (its codes; its message otherwise).
fn warning(code: WarningCode, message: &str) -> Option<String> {
    match code {
        WarningCode::LeftoversRemoved => Some(deploy_log::CLEARING_LEFTOVERS.into()),
        WarningCode::Recovering => Some(deploy_log::RECOVERING.into()),
        // A best-effort step (an old network left in place): nothing the player can act on.
        WarningCode::StepTolerated => None,
        _ => Some(message.to_owned()),
    }
}
