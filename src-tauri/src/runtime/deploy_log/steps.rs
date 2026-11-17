//! The steps of a run, as the runner says them (`isothrum_runner::Phase`): each run event of one
//! ([`DeployStep`]) carries its id, where it stands, what it is about and how far it is. The lab
//! page's run events store keeps them (`run-events.ts`), and its deploy panel builds a run's steps
//! from them, each log line filed under the step running when it came (`deploy-log.ts`).
//!
//! The runner's events reach the launcher deep in an operation (`isothrum::remote`), whose callers
//! pass its log's lines down; its steps take the task the operation runs on instead: the run
//! service runs each operation [`telling`] where its steps go, and the runner's events are
//! [`tell`]ed there. A task the operation spawns through `crate::spawn` carries them on
//! ([`carried`]); one spawned another way tells none (no operation runs a lab's run in one).

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

/// Every step id the runner says, in the order a format goes through its own: the lab page names
/// each (`DeployLog.ts`'s `STEP_IDS`, written by `ts_bindings`).
#[cfg(test)]
pub(crate) fn step_ids() -> Vec<&'static str> {
    isothrum_runner::Phase::ALL.iter().map(|p| p.id()).collect()
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

/// `work`, to be spawned, carrying the steps of the operation this task runs (none outside one):
/// what `crate::spawn` runs, so a step told in a task an operation spawns still reaches it.
pub(crate) fn carried<F>(work: F) -> impl Future<Output = F::Output> + Send + 'static
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    let steps = STEPS.try_with(Arc::clone).ok();
    async move {
        match steps {
            Some(steps) => STEPS.scope(steps, work).await,
            None => work.await,
        }
    }
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

    #[tokio::test]
    async fn a_step_told_in_a_task_the_operation_spawns_still_reaches_its_steps() {
        let told = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&told);
        let step = DeployStep { step: "apply".into(), state: DeployStepState::Started, label: None, count: None };
        telling(Arc::new(move |s: DeployStep| into.lock().unwrap().push(s.step)), async {
            tokio::spawn(carried(async move { tell(step) })).await.unwrap();
        })
        .await;
        assert_eq!(*told.lock().unwrap(), ["apply"]);
    }
}
