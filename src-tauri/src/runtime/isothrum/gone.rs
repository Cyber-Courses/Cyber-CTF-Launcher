//! A run whose surroundings changed under it: its generated files deleted while it runs, and
//! the engine it runs on stopped while a step ran (tests/resilience.md, F1 and T1).

use super::{Error, Kind, LabRun, Result, errors, lab};

impl LabRun {
    /// Whether the run has its generated files, made again when they are gone while the runner
    /// records something of the run (the player deleted the lab's `.isoloom-<n>/` folder, or a
    /// file of it, while the lab ran): without them a stop said "nothing to remove" and left the
    /// lab running, and its page read it as stopped. False for a run that never had any (a start
    /// that failed before generation: nothing to remove).
    ///
    /// # Errors
    ///
    /// When the files can't be made again (the lab's own folder or spec is gone too), saying what
    /// is left, since nothing here can reach it any more.
    pub(super) fn files_back(&self) -> Result<bool> {
        if self.manifest().is_file() {
            return Ok(true);
        }
        let recorded =
            self.runner.runs().map_err(errors::from_runner)?.into_iter().any(|r| r.name == self.name && r.state != isothrum_runner::RunState::Absent);
        if !recorded {
            return Ok(false);
        }
        log::warn!("{}: the run's files are gone while it is recorded; generating them again", self.dir.display());
        lab::prepare(&self.dir, self.target()).map(|_| true).map_err(|e| {
            Error::Invalid(format!(
                "This lab's files are gone ({e}), and what it started may still be running: Cyber CTF can't reach it any more. Remove its containers or VMs with Docker or your hypervisor (their names start with {}).",
                self.name
            ))
        })
    }

    /// Whether nothing of the run is left where it ran though the runner records it: a live read
    /// finds every machine absent (its containers removed with `docker compose down` by hand or a
    /// clean-up tool, its VMs deleted in the hypervisor's app). False when anything of it is
    /// left, even stopped, when an operation holds the run (the read isn't live), or when the run's
    /// files are gone too (nothing is made again here: [`LabRun::files_back`] is a stop's).
    ///
    /// # Errors
    ///
    /// When the run can't be read (the engine doesn't answer): whether it is gone can't be told.
    pub(crate) async fn is_gone(&self) -> Result<bool> {
        if !self.manifest().is_file() {
            return Ok(false);
        }
        let lab = self.lab().await?;
        let status = self.read(&lab).await?;
        Ok(status.live && !status.machines.is_empty() && status.machines.iter().all(|m| m.state == isothrum_runner::MachineState::Absent))
    }

    /// `e`, an operation's failure on this run, as the player reads it: on this machine's Docker,
    /// the engine stopped while a step ran is said as such ([`errors::engine_stopped`]).
    pub(super) fn said(&self, e: Error) -> Error {
        match &self.esxi {
            // On an ESXi server: a refused sign-in names it as Servers does.
            Some(place) => errors::on_server(e, &place.name),
            None if self.kind == Kind::Docker => errors::engine_stopped(e),
            None => e,
        }
    }
}
