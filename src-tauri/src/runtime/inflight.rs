//! What this process is doing to which lab right now: the per-lab operation lock, and the
//! registry of starts, stops and parks in flight that the quit guard and the UI read.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::AppHandle;
use crate::deploy_worker::lock;
use crate::error::Result;
use tokio::sync::OwnedMutexGuard;

/// What is in flight for a lab: a start (provisioning) or a stop (teardown). Told apart so the
/// UI never calls a lab being stopped "deploying".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Start,
    Stop,
    /// Pausing or shutting the lab down, machines kept: neither a deploy nor a teardown.
    Park,
}

/// The labs with an operation in flight right now, each with its in-flight count (a lab can be
/// re-entered). The app reads this to warn before quitting mid-deploy (interrupting a cloud
/// `terraform apply` leaves billable resources behind, a VM/Docker run half-created), and the UI
/// reads it to rehydrate the "this lab is starting" state after a window reload: the map lives in
/// this long-lived process, so a reloaded webview that lost its own deploy state can ask which
/// labs are still deploying and show that instead of a bare Start button.
fn in_flight() -> &'static Mutex<HashMap<String, (Action, usize)>> {
    static IN_FLIGHT: OnceLock<Mutex<HashMap<String, (Action, usize)>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Total start/stop operations in flight, across all labs.
pub fn active_deploys() -> usize {
    in_flight().lock().map(|m| m.values().map(|(_, n)| n).sum()).unwrap_or(0)
}

/// The labs being started right now (in this process), deduplicated.
pub fn deploying_labs() -> Vec<String> {
    labs_with(Action::Start)
}

/// Every in-process operation right now: the lab and what it is doing.
pub fn active_actions() -> Vec<(String, Action)> {
    in_flight().lock().map(|m| m.iter().map(|(id, (a, _))| (id.clone(), *a)).collect()).unwrap_or_default()
}

/// Whether lab `id` is being stopped in this process right now.
pub fn stopping(id: &str) -> bool {
    labs_with(Action::Stop).iter().any(|l| l == id)
}

fn labs_with(action: Action) -> Vec<String> {
    in_flight().lock().map(|m| m.iter().filter(|(_, (a, _))| *a == action).map(|(id, _)| id.clone()).collect()).unwrap_or_default()
}

/// Marks its lab as starting, stopping or parking for as long as it is alive (RAII), so the mark
/// is always cleared even if the operation errors or is cancelled.
pub(crate) struct DeployGuard(String);

impl DeployGuard {
    pub(crate) fn new(id: &str, action: Action) -> Self {
        if let Ok(mut m) = in_flight().lock() {
            let e = m.entry(id.to_string()).or_insert((action, 0));
            // The latest operation names the state (a stop right after a start is "stopping"),
            // but a stop waiting for the lab stays the state: the start it waits for doesn't
            // undo it, and nothing may begin beside it meanwhile (see `stopping`).
            if e.0 != Action::Stop {
                e.0 = action;
            }
            e.1 += 1;
        }
        Self(id.to_string())
    }
}

impl Drop for DeployGuard {
    fn drop(&mut self) {
        let Ok(mut m) = in_flight().lock() else { return };
        if let Some((_, c)) = m.get_mut(&self.0) {
            *c -= 1;
            if *c == 0 {
                m.remove(&self.0);
            }
        }
    }
}

/// A lab held for one operation: this process's own lock on it, and the lock across processes
/// (see `deploy_worker::lock`).
pub(crate) struct LabLock {
    _file: Option<crate::deploy_worker::lock::LabFileLock>,
    _here: OwnedMutexGuard<()>,
}

/// This process's lock on lab `id`.
fn process_lock(id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut map = LOCKS.get_or_init(|| Mutex::new(HashMap::new())).lock().expect("lab lock registry");
    map.entry(id.to_string()).or_default().clone()
}

/// One operation per lab at a time: install, start, stop and the expiry reaper all take this
/// per-lab lock, so a user Start/Stop and the reaper can't run two Terraform (or Vagrant)
/// operations over the same state at once, which could corrupt `terraform.tfstate`. It holds
/// across processes too: a deploy worker holds it for its whole job, and an operation of the app
/// waits for the worker (and the other way round). Not reentrant: a caller already holding it
/// uses the `_locked` variant of an operation. A lock that can't be taken fails the operation:
/// carrying on without it is what the lock is there to prevent (Terraform's own state lock is
/// cleared on the assumption that this one is held).
pub(crate) async fn lock_lab(app: &AppHandle, id: &str) -> Result<LabLock> {
    lock_lab_at(Some(&crate::deploy_worker::lock_path(app, id)?), id).await
}

/// [`lock_lab`] with its lock file at `file` (`None`: this process's lock only).
async fn lock_lab_at(file: Option<&Path>, id: &str) -> Result<LabLock> {
    let here = process_lock(id).lock_owned().await;
    let file = match file {
        Some(path) => Some(lock::acquire(path).await.map_err(lock::unavailable)?),
        None => None,
    };
    Ok(LabLock { _file: file, _here: here })
}

/// [`lock_lab`] without waiting: `None` while another operation (of this process or another)
/// holds the lab.
pub(super) fn try_lock_lab(app: &AppHandle, id: &str) -> Result<Option<LabLock>> {
    try_lock_lab_at(Some(&crate::deploy_worker::lock_path(app, id)?), id)
}

/// [`try_lock_lab`] with its lock file at `file` (`None`: this process's lock only).
fn try_lock_lab_at(file: Option<&Path>, id: &str) -> Result<Option<LabLock>> {
    let Ok(here) = process_lock(id).try_lock_owned() else { return Ok(None) };
    let file = match file {
        Some(path) => match lock::try_acquire(path).map_err(lock::unavailable)? {
            Some(held) => Some(held),
            None => return Ok(None),
        },
        None => None,
    };
    Ok(Some(LabLock { _file: file, _here: here }))
}

#[cfg(test)]
mod tests {
    use super::{Action, DeployGuard, active_actions, active_deploys, deploying_labs, stopping, try_lock_lab_at};

    async fn lock_lab(id: &str) -> super::LabLock {
        super::lock_lab_at(None, id).await.unwrap()
    }

    fn try_lock(file: Option<&std::path::Path>, id: &str) -> Option<super::LabLock> {
        try_lock_lab_at(file, id).unwrap()
    }

    #[test]
    fn every_guard_counts_as_an_operation_in_flight() {
        // Other tests hold guards of their own meanwhile: only a lower bound holds.
        let a = DeployGuard::new("inflight-test-count-a", Action::Start);
        let b = DeployGuard::new("inflight-test-count-b", Action::Stop);
        assert!(active_deploys() >= 2);
        drop((a, b));
    }

    // The registry is process-wide: each test uses its own lab ids and only looks at those.
    fn action_of(id: &str) -> Option<Action> {
        active_actions().into_iter().find(|(l, _)| l == id).map(|(_, a)| a)
    }

    #[test]
    fn guard_marks_the_lab_until_the_last_one_drops() {
        let id = "inflight-test-nested";
        let first = DeployGuard::new(id, Action::Start);
        assert!(deploying_labs().contains(&id.to_string()));
        // A stop entered while the start is still in flight names the state.
        let second = DeployGuard::new(id, Action::Stop);
        assert_eq!(action_of(id), Some(Action::Stop));
        assert!(!deploying_labs().contains(&id.to_string()));
        drop(second);
        assert_eq!(action_of(id), Some(Action::Stop), "still one operation in flight");
        drop(first);
        assert_eq!(action_of(id), None);
    }

    #[test]
    fn park_is_neither_a_deploy_nor_a_teardown() {
        let id = "inflight-test-park";
        let g = DeployGuard::new(id, Action::Park);
        assert_eq!(action_of(id), Some(Action::Park));
        assert!(!deploying_labs().contains(&id.to_string()));
        drop(g);
        assert_eq!(action_of(id), None);
    }

    #[test]
    fn a_guard_dropped_by_a_panic_clears_its_mark() {
        // An operation that panics (a bug in a runtime step) unwinds through its guard: the lab
        // must not stay "starting" for the rest of the app's life, holding every quit.
        let id = "inflight-test-panic";
        let crashed = std::thread::spawn(move || {
            let _guard = DeployGuard::new(id, Action::Start);
            assert!(deploying_labs().contains(&id.to_string()));
            panic!("a step blew up");
        })
        .join();
        assert!(crashed.is_err(), "the thread panicked");
        assert_eq!(action_of(id), None, "the mark went with the guard");
        // The registry still works afterwards (its mutex wasn't poisoned by the unwind).
        let again = DeployGuard::new(id, Action::Park);
        assert_eq!(action_of(id), Some(Action::Park));
        drop(again);
        assert_eq!(action_of(id), None);
    }

    #[test]
    fn nested_guards_release_the_lab_only_when_the_outermost_one_drops() {
        // Three operations re-entering the same lab (a start that resumes, which parks the
        // attacker...): the mark holds while any of them is alive, whatever the drop order.
        let id = "inflight-test-nested-three";
        let a = DeployGuard::new(id, Action::Start);
        let b = DeployGuard::new(id, Action::Start);
        let c = DeployGuard::new(id, Action::Start);
        drop(a);
        assert_eq!(action_of(id), Some(Action::Start), "the outermost dropped first");
        drop(c);
        assert_eq!(action_of(id), Some(Action::Start));
        drop(b);
        assert_eq!(action_of(id), None);
    }

    #[tokio::test]
    async fn try_lock_steps_aside_for_a_holder_in_this_process() {
        // The reaper's non-waiting lock sees an in-process operation (no lock file needed).
        let id = "inflight-test-try-here";
        let held = lock_lab(id).await;
        assert!(try_lock(None, id).is_none());
        drop(held);
        let mine = try_lock(None, id);
        assert!(mine.is_some());
        assert!(try_lock(None, id).is_none(), "not reentrant");
        drop(mine);
        assert!(try_lock(None, id).is_some());
    }

    #[tokio::test]
    async fn a_lock_that_cant_be_taken_fails_the_operation() {
        // Regression: a lock file that couldn't be opened was logged and the operation carried
        // on unlocked, beside Terraform runs that clear their own state lock assuming it held.
        let file = std::env::temp_dir().join("cyberctf-never-created").join("inflight-test-unlockable.lock");
        let waited = super::lock_lab_at(Some(&file), "inflight-test-unlockable").await;
        assert!(waited.err().unwrap().to_string().starts_with("Couldn't lock the lab"));
        let tried = try_lock_lab_at(Some(&file), "inflight-test-unlockable");
        assert!(tried.err().unwrap().to_string().starts_with("Couldn't lock the lab"));
        // Its in-process lock went with the failure: the next operation isn't held up.
        assert!(try_lock(None, "inflight-test-unlockable").is_some());
    }

    #[test]
    fn a_lab_is_stopping_while_a_stop_guard_lives() {
        let id = "inflight-test-stopping";
        assert!(!stopping(id));
        let start = DeployGuard::new(id, Action::Start);
        assert!(!stopping(id));
        let stop = DeployGuard::new(id, Action::Stop);
        assert!(stopping(id) && !stopping("inflight-test-stopping-other"));
        // A launch that held the lab reaches its start while the stop waits: still stopping.
        let launch = DeployGuard::new(id, Action::Start);
        assert!(stopping(id), "a pending stop isn't undone by the start it waits for");
        drop((start, launch, stop));
        assert!(!stopping(id));
    }

    #[tokio::test]
    async fn lab_lock_is_per_lab() {
        let a = lock_lab("inflight-test-lock-a").await;
        // Another lab isn't held up by it.
        let b = tokio::time::timeout(std::time::Duration::from_secs(1), lock_lab("inflight-test-lock-b")).await;
        assert!(b.is_ok());
        // The same lab waits until the holder lets go.
        let again = tokio::time::timeout(std::time::Duration::from_millis(50), lock_lab("inflight-test-lock-a")).await;
        assert!(again.is_err());
        drop(a);
        assert!(tokio::time::timeout(std::time::Duration::from_secs(1), lock_lab("inflight-test-lock-a")).await.is_ok());
    }

    #[tokio::test]
    async fn a_lab_held_by_another_process_is_waited_for_or_skipped() {
        let dir = std::env::temp_dir().join(format!("cyberctf-lablock-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("inflight-test-other.lock");
        // Another process's hold on the lab (a deploy worker), as a handle of its own.
        let worker = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&file).unwrap();
        worker.lock().unwrap();
        assert!(try_lock(Some(&file), "inflight-test-other").is_none(), "the reaper steps aside");
        let waiting = tokio::time::timeout(std::time::Duration::from_millis(600), super::lock_lab_at(Some(&file), "inflight-test-other")).await;
        assert!(waiting.is_err(), "a stop waits for the worker");
        drop(worker);
        let mut held = None;
        assert!(
            crate::deploy_worker::lock::tests::eventually(|| {
                held = try_lock(Some(&file), "inflight-test-other");
                held.is_some()
            }),
            "free once the worker is gone"
        );
        // Held here now: a worker can't take it.
        assert!(crate::deploy_worker::lock::held(&file));
        drop(held);
        assert!(crate::deploy_worker::lock::tests::eventually(|| !crate::deploy_worker::lock::held(&file)));
        std::fs::remove_dir_all(dir).ok();
    }
}
