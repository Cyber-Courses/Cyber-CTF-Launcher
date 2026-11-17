//! The steps of a run, as the runner says them (`isothrum_runner::Phase`): each run event of one
//! ([`DeployStep`]) carries its id, where it stands, what it is about and how far it is. The lab
//! page builds a run's steps from them and files its log's lines under the step they came in
//! (`deploy-log.ts`), so it never reads a step out of a tool's words.
//!
//! The runner's events reach the launcher deep in an operation (`isothrum::remote`), whose callers
//! pass its log's lines down; its steps take the task the operation runs on instead: the run
//! service runs each operation [`telling`] where its steps go, and the runner's events are
//! [`tell`]ed there.

use std::sync::Arc;

use serde::Serialize;

/// One step of a run, as the runner said it.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DeployStep {
    /// Its id: one of the runner's steps (`images`, `up`, `apply`, …: `isothrum_runner::Phase::id`).
    pub step: String,
    /// Where it stands.
    pub state: DeployStepState,
    /// What it is about, when it is one of several of its kind: a machine.
    pub label: Option<String>,
    /// How far it is, when it counts what it does (images, containers, boxes, resources).
    pub count: Option<StepCount>,
}

/// Where a step stands.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum DeployStepState {
    /// Started, or moved on (said again with its new count).
    Started,
    /// Finished and succeeded.
    Done,
    /// Finished and failed: the run's failure is this step's.
    Failed,
    /// Cut short or never run: another step failed.
    Skipped,
}

/// How far a step is: `done` of its `total` items, once the total is known.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StepCount {
    /// The items done so far.
    pub done: u32,
    /// How many there are.
    pub total: Option<u32>,
}

/// Where the steps of the operation a task runs go.
pub(crate) type Steps = Arc<dyn Fn(DeployStep) + Send + Sync>;

tokio::task_local! {
    static STEPS: Steps;
}

/// Runs `work`, the steps its runs say told to `steps`.
pub(crate) async fn telling<T>(steps: Steps, work: impl Future<Output = T>) -> T {
    STEPS.scope(steps, work).await
}

/// Tells `step` where the steps of the operation this task runs go; nowhere outside one (a
/// command line's own run).
pub(crate) fn tell(step: DeployStep) {
    let _ = STEPS.try_with(|steps| steps(step));
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[tokio::test]
    async fn a_step_told_in_an_operation_reaches_its_steps_and_one_told_outside_none() {
        let told = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&told);
        let step = |id: &str| DeployStep { step: id.into(), state: DeployStepState::Started, label: None, count: None };
        tell(step("before"));
        telling(Arc::new(move |s: DeployStep| into.lock().unwrap().push(s.step)), async {
            tokio::task::yield_now().await;
            tell(step("images"));
        })
        .await;
        tell(step("after"));
        assert_eq!(*told.lock().unwrap(), ["images"]);
    }
}
