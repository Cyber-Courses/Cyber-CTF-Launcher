//! The runner's events as the launcher's progress: the lines of a deploy log (the lab page shows
//! them as they come, and the sidebar takes its step from the last one) and its steps (the lab
//! page's steps are built from them). What a tool prints and what the runner warns about are
//! lines; state moves, outputs and the final result or error are neither (the caller says how it
//! ended).

use super::super::deploy_log::{self, DeployStep, DeployStepState, StepCount};
use isothrum_runner::{EventKind, StepState, WarningCode};

/// The deploy log's line for an event, if it gets one.
pub(super) fn line(kind: &EventKind) -> Option<String> {
    match kind {
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

/// The deploy log's step for an event, if it is one of the runner's steps.
pub(super) fn step(kind: &EventKind) -> Option<DeployStep> {
    let EventKind::Step { step, state, label, count } = kind else { return None };
    let state = match state {
        StepState::Done => DeployStepState::Done,
        StepState::Failed => DeployStepState::Failed,
        StepState::Skipped => DeployStepState::Skipped,
        // A state a newer runner says moves the step on, as a started one does.
        _ => DeployStepState::Started,
    };
    Some(DeployStep { step: step.clone(), state, label: label.clone(), count: count.map(|c| StepCount { done: c.done, total: c.total }) })
}
