//! What this process is doing to which lab right now: the per-lab operation lock, and the
//! registry of starts, stops and parks in flight that the quit guard and the UI read.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

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

/// One operation per lab at a time: start, stop and the expiry reaper all take this per-lab
/// lock, so a user Start/Stop and the reaper can't run two Terraform (or Vagrant) operations
/// over the same state at once, which could corrupt `terraform.tfstate`. Not reentrant: a caller
/// already holding it uses the `_locked` variant of an operation.
///
/// It only serialises this process. A deploy worker is another process with a lock of its own,
/// so the reaper also skips a lab with a live worker, and a worker is only spawned under this
/// lock (`deploy_worker::run_job`): the reaper's check and its teardown can't interleave a spawn.
pub(crate) async fn lock_lab(id: &str) -> OwnedMutexGuard<()> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let lock = {
        let mut map = LOCKS.get_or_init(|| Mutex::new(HashMap::new())).lock().expect("lab lock registry");
        map.entry(id.to_string()).or_default().clone()
    };
    lock.lock_owned().await
}

#[cfg(test)]
mod tests {
    use super::{Action, DeployGuard, active_actions, active_deploys, deploying_labs, lock_lab, parking_labs, stopping_labs};

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
}
