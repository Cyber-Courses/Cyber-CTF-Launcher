//! The lab operations in flight, from both places they run: the home's runner service (every
//! operation on a lab's run) and this process (the launcher's own steps around them, a stop, the
//! attack box): what an update install waits for. The webview reads the operations from the run
//! events (`run_service`), never from here.

use crate::platform::AppDirs;
use crate::runtime;

/// The labs with any operation in flight: every one of this process ([`runtime::busy_labs`]: a
/// start, a provisioning run, a stop, a park), then those whose run has an operation queued or
/// running in the runner service. Installing an update quits every Cyber CTF process, the
/// service included, so it waits while this isn't empty: a stop cut off mid-`terraform destroy`
/// leaves billable resources behind as surely as a launch.
///
/// # Errors
///
/// The runner service's runs can't be listed: not "nothing in flight", which the update install
/// would take as leave to quit them all.
pub async fn busy_labs(dirs: AppDirs) -> Result<Vec<String>, String> {
    let served = runtime::isothrum::busy_in_service(&dirs).await?;
    Ok(merge_unique(runtime::busy_labs(), served))
}

/// `ids` followed by each of `more` not already in it, in order.
fn merge_unique(mut ids: Vec<String>, more: impl IntoIterator<Item = String>) -> Vec<String> {
    for id in more {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Action, Operation, Park};

    /// A command's answer, from a test that isn't async.
    fn wait<T>(answer: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(answer)
    }

    #[test]
    fn the_services_lab_ids_are_added_once_after_the_in_app_ones() {
        assert_eq!(merge_unique(vec!["a".into(), "b".into()], ["b".to_owned(), "c".to_owned()]), ["a", "b", "c"]);
    }

    #[test]
    fn a_stop_or_a_park_in_this_process_keeps_its_lab_busy() {
        // What an update install waits for: quitting mid-`terraform destroy` leaves billable
        // resources behind.
        let t = crate::testing::test_app::TestApp::new();
        let busy = |id: &str| wait(busy_labs(t.dirs())).unwrap().iter().any(|l| l == id);
        for (id, action) in [("operations-test-busy-stop", Action::Stop), ("operations-test-busy-park", Action::Park(Park::Pause))] {
            assert!(!busy(id));
            let guard = crate::runtime::DeployGuard::new(id, action);
            assert!(busy(id), "{action:?} isn't counted");
            drop(guard);
            assert!(!busy(id));
        }
    }

    #[test]
    fn operations_get_their_sidebar_names() {
        // A shutdown isn't a pause, a resume or a provisioning run not a launch.
        assert_eq!(Action::Launch.operation(), Operation::Launch);
        assert_eq!(Action::Stop.operation(), Operation::Stop);
        assert_eq!(Action::Park(Park::Pause).operation(), Operation::Pause);
        assert_eq!(Action::Park(Park::Shutdown).operation(), Operation::Shutdown);
        assert_eq!(Action::Resume.operation(), Operation::Resume);
        assert_eq!(Action::Provision.operation(), Operation::Provision);
    }
}
