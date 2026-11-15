//! What the launcher's own run service watches, and how it reconciles it with what runs: each
//! pass scans this machine (Docker, the hypervisors) and reads the status of the labs that may
//! have changed, telling only what did. The webview reads the statuses from these events, never
//! by polling; the runner's service does the same for its runs.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::error::Wire;
use crate::platform::AppDirs;
use crate::runtime::{LabStatus, Runtime};

/// A status read: the status, or why it couldn't be read.
pub(super) type Read = std::result::Result<LabStatus, Wire>;

/// Every how many passes every watched lab is read, whatever it looks like (one that changed
/// outside the app with nothing on this machine to show it: a cloud lab expired, a server's).
const ALL_EVERY: u64 = 10;

/// What is watched, and what was told of it.
#[derive(Default)]
pub(super) struct Watch {
    /// The labs watched, with their catalogue runtimes.
    labs: BTreeMap<String, Runtime>,
    /// The labs looked at closely (a page shows them): read at every pass.
    open: BTreeSet<String>,
    /// The last read told of each lab.
    told: HashMap<String, Read>,
    /// What the last scan saw up; none before the first.
    scan: Option<BTreeSet<String>>,
    /// The labs read at the last pass: one that drops out is read once more, to settle.
    read_last: BTreeSet<String>,
    /// The passes so far.
    passes: u64,
}

impl Watch {
    /// Watches `labs` and `open` from now on, in place of what was; the labs newly watched, to
    /// read at once.
    pub(super) fn set(&mut self, labs: Vec<(String, Runtime)>, open: Vec<String>) -> Vec<(String, Runtime)> {
        let fresh = labs.iter().filter(|(id, rt)| self.labs.get(id) != Some(rt)).cloned().collect();
        self.labs = labs.into_iter().collect();
        self.open = open.into_iter().collect();
        self.told.retain(|id, _| self.labs.contains_key(id));
        fresh
    }

    /// Records what the scan sees up: whether it changed (the first scan counts as a change).
    pub(super) fn scanned(&mut self, seen: BTreeSet<String>) -> bool {
        let changed = self.scan.as_ref() != Some(&seen);
        self.scan = Some(seen);
        changed
    }

    /// The labs to read at this pass: those that can change without the app doing anything (the
    /// scan sees them up, or they ran last time it read them), those an operation acts on
    /// (`busy`), those looked at closely, those read last time (once more, to settle), and every
    /// watched lab every [`ALL_EVERY`] passes.
    pub(super) fn next_reads(&mut self, busy: impl Fn(&str) -> bool) -> Vec<(String, Runtime)> {
        self.passes += 1;
        let all = self.passes % ALL_EVERY == 1;
        let scan = self.scan.clone().unwrap_or_default();
        let ran = |id: &str| self.told.get(id).is_some_and(|r| r.as_ref().is_ok_and(|s| s.running));
        let hot: BTreeSet<String> = self
            .labs
            .keys()
            .filter(|id| all || scan.contains(*id) || ran(id) || busy(id) || self.open.contains(*id) || self.read_last.contains(*id))
            .cloned()
            .collect();
        let pick = hot.iter().filter_map(|id| self.labs.get(id).map(|rt| (id.clone(), *rt))).collect();
        self.read_last = hot.into_iter().filter(|id| scan.contains(id) || ran(id) || busy(id) || self.open.contains(id)).collect();
        pick
    }

    /// Records `read` of lab `id`: whether it says something new (a lab no longer watched says
    /// nothing).
    pub(super) fn told(&mut self, id: &str, read: &Read) -> bool {
        if !self.labs.contains_key(id) || self.told.get(id) == Some(read) {
            return false;
        }
        self.told.insert(id.to_owned(), read.clone());
        true
    }
}

/// Lab `id`, declared `runtime`, as it looks right now ([`crate::commands::labs::status_in`]), as
/// a read to tell.
pub(super) async fn read(dirs: &AppDirs, id: &str, runtime: Runtime) -> Read {
    Box::pin(crate::commands::labs::status_in(dirs, id, runtime)).await.map_err(|e| e.to_wire())
}

#[cfg(test)]
mod tests {
    use super::{BTreeSet, LabStatus, Read, Runtime, Watch};

    fn labs(ids: &[&str]) -> Vec<(String, Runtime)> {
        ids.iter().map(|id| ((*id).to_owned(), Runtime::Docker)).collect()
    }

    fn ids(picked: &[(String, Runtime)]) -> Vec<&str> {
        picked.iter().map(|(id, _)| id.as_str()).collect()
    }

    const UP: fn() -> Read = || Ok(LabStatus { running: true, ..LabStatus::default() });

    #[test]
    fn the_first_pass_reads_every_lab_then_only_those_that_can_change() {
        let mut w = Watch::default();
        assert_eq!(ids(&w.set(labs(&["a", "b", "c", "d"]), vec!["d".into()])), ["a", "b", "c", "d"], "all new");
        w.scanned(BTreeSet::from(["a".to_owned()]));
        assert_eq!(ids(&w.next_reads(|_| false)), ["a", "b", "c", "d"], "every lab at the first pass");
        assert!(w.told("b", &UP()));
        // a: the scan sees it; b: it ran last time; c: busy; d: its page is open.
        assert_eq!(ids(&w.next_reads(|id| id == "c")), ["a", "b", "c", "d"]);
        assert!(w.told("b", &Ok(LabStatus::default())));
        // c's operation ended: read once more, then left until the next full pass.
        assert_eq!(ids(&w.next_reads(|_| false)), ["a", "b", "c", "d"]);
        assert_eq!(ids(&w.next_reads(|_| false)), ["a", "d"]);
    }

    #[test]
    fn only_a_change_is_told_and_only_of_a_watched_lab() {
        let mut w = Watch::default();
        w.set(labs(&["a"]), Vec::new());
        assert!(w.told("a", &UP()));
        assert!(!w.told("a", &UP()), "the same again");
        assert!(w.told("a", &Ok(LabStatus::default())));
        assert!(!w.told("zz", &UP()), "not watched");
        assert!(w.scanned(BTreeSet::new()), "the first scan");
        assert!(!w.scanned(BTreeSet::new()));
        assert!(w.scanned(BTreeSet::from(["a".to_owned()])));
        // Watched again with the same runtime: nothing new to read; another runtime is.
        assert_eq!(w.set(labs(&["a"]), Vec::new()), [] as [(String, Runtime); 0]);
        assert_eq!(w.set(vec![("a".into(), Runtime::Vm)], Vec::new()).len(), 1);
    }
}
