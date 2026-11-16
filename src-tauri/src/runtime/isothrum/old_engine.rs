//! The engine an earlier version noted beside a container lab it started on a Docker context other
//! than the current one (`.cyberctf-engine` in the lab's folder): the runner records a run's
//! engine itself now, but a run an earlier version started has none recorded. Until the run's
//! first operation records it, its context is given to every operation and read; once one went
//! through, the runner keeps it and the note goes.

use std::path::Path;

/// The note an earlier version wrote: the context's name.
pub(super) const MARKER: &str = ".cyberctf-engine";

/// The context the lab in `dir` was started on, as an earlier version noted it; none without a
/// note, or a blank one.
pub(super) fn noted(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(MARKER)).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// Removes the note in `dir`, the runner now recording the run's engine; one that can't be
/// removed is said in the log, and read again (harmlessly) next time.
pub(super) fn forget(dir: &Path) {
    if let Err(e) = crate::platform::fs::remove_if_present(&dir.join(MARKER)) {
        log::warn!("{}: the earlier version's engine note couldn't be removed: {e}", dir.display());
    }
}
