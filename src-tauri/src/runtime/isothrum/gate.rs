//! What each operation needs of the run's recorded state before it is asked of the runner: the
//! runner runs any operation it is given whatever state the run is in (an `up` of a running run
//! applies it again, a resume of a run of which nothing is left makes it afresh with no launch
//! values, a check of a run never started starts its services), so the launcher refuses those
//! itself, with the words the runner's own refusals have.

use isothrum_runner::{Error as RunnerError, RunState};

use super::{LabRun, Result, errors, paths};

impl LabRun {
    /// The run's recorded state, none before it ever ran: read from the runner's store, no tool runs.
    fn recorded_state(&self) -> Result<Option<RunState>> {
        Ok(self.runner.run_record(&self.name).map_err(errors::from_runner)?.map(|r| r.state))
    }

    /// Refuses a start of a run that is up already: Stop it first.
    pub(super) fn refuse_up_when_running(&self) -> Result<()> {
        match self.recorded_state()? {
            Some(RunState::Ready) => Err(errors::from_runner(RunnerError::AlreadyRunning { run: self.name.to_string() })),
            _ => Ok(()),
        }
    }

    /// Refuses a change to a running run (an update, a machine taken out) unless it is up, every
    /// machine running, as the runner's own rule for an update.
    pub(super) fn require_up(&self) -> Result<()> {
        match self.recorded_state()? {
            Some(RunState::Ready) => Ok(()),
            _ => Err(self.not_running()),
        }
    }

    /// Refuses a check of a run of which nothing runs: never started, taken down or parked. A run
    /// with a machine down (`failed`) is checked: its checks say what doesn't answer.
    pub(super) fn require_started(&self) -> Result<()> {
        match self.recorded_state()? {
            Some(RunState::Ready | RunState::Failed) => Ok(()),
            _ => Err(self.not_running()),
        }
    }

    /// The runner's words for a run that isn't running.
    fn not_running(&self) -> crate::error::Error {
        errors::from_runner(RunnerError::NotRunning { run: self.name.to_string() })
    }

    /// Refuses a resume when nothing is left of the run to start again (never started, taken down,
    /// or every machine removed outside the app): the parked mark goes, so the lab page offers a
    /// fresh start, which takes the launch's values, instead.
    pub(super) async fn require_something_to_resume(&self) -> Result<()> {
        let gone = match self.recorded_state()? {
            None | Some(RunState::Absent) => true,
            Some(_) => self.is_gone().await?,
        };
        if gone {
            paths::mark_parked(&self.dir, None)?;
            return Err(errors::from_runner(RunnerError::NothingToResume { run: self.name.to_string() }));
        }
        Ok(())
    }
}
