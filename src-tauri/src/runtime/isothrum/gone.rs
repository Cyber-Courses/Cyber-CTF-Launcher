//! A run whose surroundings changed under it: its generated files deleted while it runs, and
//! the engine it runs on stopped while a step ran (docs/contributing/testing/resilience.md, F1 and T1).

use super::{Error, LabRun, Result, errors, lab};

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
    pub(super) async fn files_back(&self) -> Result<bool> {
        if self.has_project() {
            return Ok(true);
        }
        let recorded = self.recorded().await?.is_some_and(|r| r.state != isothrum_runner::RunState::Absent);
        if !recorded {
            return Ok(false);
        }
        log::warn!("{}: the run's files are gone while it is recorded; generating them again", self.dir.display());
        // A helper VM an earlier version left has no target to generate again: Isoloom dropped it.
        let target = self.target().ok_or_else(|| Error::Invalid("Isoloom no longer generates Docker inside a VM".into()));
        target.and_then(|t| lab::prepare(&self.dir, t)).map(|_| true).map_err(|e| {
            Error::Invalid(format!(
                "This lab's files are gone ({e}), and what it started may still be running: Cyber CTF can't reach it any more. Remove its containers or VMs with Docker or your hypervisor (their names start with {}).",
                self.served.name
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
        if !self.has_project() {
            return Ok(false);
        }
        let status = self.read().await?;
        // A live read lists what it found: none, or only machines absent, is nothing left.
        Ok(status.live && status.machines.iter().all(|m| m.state == isothrum_runner::MachineState::Absent))
    }

    /// `e`, an operation's failure on this run, as the player reads it: on an ESXi server, a
    /// refused sign-in names the server as Servers does.
    pub(super) fn said(&self, e: Error) -> Error {
        match &self.esxi {
            Some(place) => errors::on_server(e, &place.name),
            None => e,
        }
    }
}
