//! What each operation needs of the run's recorded state before it is asked of the runner, where
//! the runner doesn't refuse it itself: an update of a run that isn't up, a check of a run of which
//! nothing runs (it would start its services), a resume of a run of which nothing is left (it would
//! make it afresh with no launch values). The runner refuses a second `up` of a running run and a
//! resume of a run it never recorded itself; the launcher's refusals take the words of the runner's.

use isothrum_runner::{Error as RunnerError, RunState};

use super::{LabRun, Result, errors};

impl LabRun {
    /// The run's recorded state, none before it ever ran: read from the runner's store, no tool runs.
    fn recorded_state(&self) -> Result<Option<RunState>> {
        Ok(self.runner.run_record(&self.name).map_err(|e| errors::coded(&e.report()))?.map(|r| r.state))
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
        errors::coded(&RunnerError::NotRunning { run: self.name.to_string() }.report())
    }

    /// Refuses a resume when nothing is left of the run to start again (never started, taken down,
    /// or every machine removed outside the app), so the lab page offers a fresh start, which
    /// takes the launch's values, instead.
    pub(super) async fn require_something_to_resume(&self) -> Result<()> {
        let gone = match self.recorded_state()? {
            None | Some(RunState::Absent) => true,
            Some(_) => self.is_gone().await?,
        };
        if gone {
            return Err(errors::coded(&RunnerError::NothingToResume { run: self.name.to_string() }.report()));
        }
        Ok(())
    }
}
