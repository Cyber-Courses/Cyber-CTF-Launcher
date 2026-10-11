//! What this process is doing to which lab right now: the per-lab operation lock, and the
//! registry of starts, stops and parks in flight that the quit guard and the UI read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tauri::AppHandle;
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

/// The labs being stopped right now (in this process), deduplicated.
pub fn stopping_labs() -> Vec<String> {
    labs_with(Action::Stop)
}

/// The labs being paused or shut down right now (in this process), deduplicated.
pub fn parking_labs() -> Vec<String> {
    labs_with(Action::Park)
}

/// Every in-process operation right now: the lab and what it is doing.
pub fn active_actions() -> Vec<(String, Action)> {
    in_flight().lock().map(|m| m.iter().map(|(id, (a, _))| (id.clone(), *a)).collect()).unwrap_or_default()
}

fn labs_with(action: Action) -> Vec<String> {
    in_flight().lock().map(|m| m.iter().filter(|(_, (a, _))| *a == action).map(|(id, _)| id.clone()).collect()).unwrap_or_default()
}

/// Marks its lab as starting, stopping or parking for as long as it is alive (RAII), so the mark
/// is always cleared even if the operation errors or is cancelled.
pub(super) struct DeployGuard(String);

impl DeployGuard {
    pub(super) fn new(id: &str, action: Action) -> Self {
        if let Ok(mut m) = in_flight().lock() {
            let e = m.entry(id.to_string()).or_insert((action, 0));
            // The latest operation names the state (a stop right after a start is "stopping").
            e.0 = action;
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
/// (see `deploy_worker::lock`), when its file could be opened.
pub(super) struct LabLock {
    _file: Option<crate::deploy_worker::lock::LabFileLock>,
    _here: OwnedMutexGuard<()>,
}

/// This process's lock on lab `id`.
fn process_lock(id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut map = LOCKS.get_or_init(|| Mutex::new(HashMap::new())).lock().expect("lab lock registry");
    map.entry(id.to_string()).or_default().clone()
}

/// The lab's lock file, if its folder can be had.
fn lock_file(app: &AppHandle, id: &str) -> Option<PathBuf> {
    crate::deploy_worker::lock_path(app, id).map_err(|e| log::warn!("lab lock for {id}: {e}")).ok()
}

/// One operation per lab at a time: start, stop and the expiry reaper all take this per-lab
/// lock, so a user Start/Stop and the reaper can't run two Terraform (or Vagrant) operations
/// over the same state at once, which could corrupt `terraform.tfstate`. It holds across
/// processes too: a deploy worker holds it for its whole job, and an operation of the app waits
/// for the worker (and the other way round). Not reentrant: a caller already holding it uses the
/// `_locked` variant of an operation.
pub(super) async fn lock_lab(app: &AppHandle, id: &str) -> LabLock {
    lock_lab_at(lock_file(app, id).as_deref(), id).await
}

/// [`lock_lab`] with its lock file at `file`.
async fn lock_lab_at(file: Option<&Path>, id: &str) -> LabLock {
    let here = process_lock(id).lock_owned().await;
    let file = match file {
        Some(path) => crate::deploy_worker::lock::acquire(path).await.map_err(|e| log::warn!("lab lock for {id}: {e}")).ok(),
        None => None,
    };
    LabLock { _file: file, _here: here }
}

/// [`lock_lab`] without waiting: `None` while another operation (of this process or another)
/// holds the lab.
pub(super) fn try_lock_lab(app: &AppHandle, id: &str) -> Option<LabLock> {
    try_lock_lab_at(lock_file(app, id).as_deref(), id)
}

/// [`try_lock_lab`] with its lock file at `file`.
fn try_lock_lab_at(file: Option<&Path>, id: &str) -> Option<LabLock> {
    let here = process_lock(id).try_lock_owned().ok()?;
    let file = match file.map(crate::deploy_worker::lock::try_acquire) {
        Some(Ok(Some(lock))) => Some(lock),
        Some(Ok(None)) => return None,
        Some(Err(e)) => {
            log::warn!("lab lock for {id}: {e}");
            None
        }
        None => None,
    };
    Some(LabLock { _file: file, _here: here })
}

#[cfg(test)]
mod tests {
    use super::{Action, DeployGuard, active_actions, active_deploys, deploying_labs, parking_labs, stopping_labs, try_lock_lab_at};

    async fn lock_lab(id: &str) -> super::LabLock {
        super::lock_lab_at(None, id).await
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
        assert!(stopping_labs().contains(&id.to_string()));
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
        assert!(parking_labs().contains(&id.to_string()));
        assert!(!deploying_labs().contains(&id.to_string()));
        assert!(!stopping_labs().contains(&id.to_string()));
        drop(g);
        assert!(!parking_labs().contains(&id.to_string()));
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
        assert!(try_lock_lab_at(Some(&file), "inflight-test-other").is_none(), "the reaper steps aside");
        let waiting = tokio::time::timeout(std::time::Duration::from_millis(600), super::lock_lab_at(Some(&file), "inflight-test-other")).await;
        assert!(waiting.is_err(), "a stop waits for the worker");
        drop(worker);
        let held = try_lock_lab_at(Some(&file), "inflight-test-other").expect("free once the worker is gone");
        // Held here now: a worker can't take it.
        assert!(crate::deploy_worker::lock::held(&file));
        drop(held);
        assert!(!crate::deploy_worker::lock::held(&file));
        std::fs::remove_dir_all(dir).ok();
    }
}
