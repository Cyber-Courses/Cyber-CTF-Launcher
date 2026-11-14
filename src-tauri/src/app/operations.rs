//! The lab operations in flight, from both places they run: detached deploy workers and this
//! process (a stop, the attack box, or the fallback when no worker could start): what an update
//! install waits for, and how a worker's operation is named when the run service picks it up.
//! The webview reads the operations from the run events (`run_service`), never from here.

use super::deploy_worker::{self, Op};
use crate::platform::AppDirs;
use crate::runtime::{self, Action, Operation, Runtime};

/// The labs with any operation in flight: every one of this process ([`runtime::busy_labs`]: a
/// start, a provisioning run, a stop, a park), then those a deploy worker runs. Installing an
/// update quits every Cyber CTF process, the workers included, so it waits while this isn't
/// empty: a stop cut off mid-`terraform destroy` leaves billable resources behind as surely as a
/// launch.
pub async fn busy_labs(dirs: AppDirs) -> Result<Vec<String>, String> {
    // Not `or_logged`: an answer that couldn't be read is not "nothing in flight", and the update
    // install, which quits every process, must not read it so: a default here would hide the
    // failure.
    let workers = off_main(move || deploy_worker::running(&dirs).map_err(|e| e.to_string())).await??;
    Ok(merge_unique(runtime::busy_labs(), workers))
}

/// Runs `f` on a blocking thread: each look at a worker asks the OS about its process (with
/// retries on macOS), which must not stall the main thread.
async fn off_main<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())
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

/// The sidebar's name for a worker's operation (its [`Action`]'s), and the machine it targets
/// when one.
pub(crate) fn worker_op(op: &Op) -> (Operation, Option<String>) {
    match op {
        Op::Launch | Op::LaunchLocal { .. } => (Action::Launch.operation(), None),
        Op::Resume { .. } => (Action::Resume.operation(), None),
        Op::Park { mode, .. } => (Action::Park(*mode).operation(), None),
        Op::Provision { machine, .. } => (Action::Provision.operation(), machine.clone()),
    }
}

/// The runtime a worker's job runs its lab in: a launch's payload names it (`startLab`'s
/// `runtime`), a lab imported from a folder runs as containers, the other operations carry it.
pub(crate) fn job_runtime(job: &deploy_worker::Job) -> Option<Runtime> {
    match &job.op {
        Op::Launch => job.launch.as_ref().map(|l| l.launch.runtime),
        Op::LaunchLocal { .. } => Some(Runtime::Docker),
        Op::Resume { runtime } | Op::Park { runtime, .. } | Op::Provision { runtime, .. } => Some(*runtime),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Park;

    #[test]
    fn worker_lab_ids_are_added_once_after_the_in_app_ones() {
        let ids = merge_unique(vec!["b".into(), "a".into()], ["a".to_string(), "c".to_string(), "c".to_string()]);
        assert_eq!(ids, ["b", "a", "c"]);
    }

    #[test]
    fn a_workers_runtime_is_its_launchs_or_its_operations() {
        // A launch's runtime is its payload's; a lab from a folder runs as containers.
        let spec =
            crate::labs::parse_launch(serde_json::json!({ "labId": "goad", "runtime": "VM", "repository": "CyberCTF/goad", "commit": "abc", "env": [] }))
                .unwrap();
        let launch = deploy_worker::Job {
            launch: Some(deploy_worker::LaunchJob { launch: spec, place: crate::runtime::Placement::default() }),
            ..deploy_worker::Job::on_lab("goad", Op::Launch)
        };
        assert_eq!(job_runtime(&launch), Some(Runtime::Vm));
        assert_eq!(job_runtime(&deploy_worker::Job::on_lab("mine", Op::LaunchLocal { src: "/labs/mine".into() })), Some(Runtime::Docker));
        assert_eq!(job_runtime(&deploy_worker::Job::on_lab("ad", Op::Resume { runtime: Runtime::Vm })), Some(Runtime::Vm));
    }

    /// A command's answer, from a test that isn't async.
    fn wait<T>(answer: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(answer)
    }

    /// Waits (bounded) for the stand-in process `pid` to have exec'd into a recognisable worker,
    /// rather than sleeping a fixed time for it: on a loaded runner the exec took longer than the
    /// sleep, the process read as not a worker, and these tests flaked under CI.
    #[cfg(unix)]
    fn wait_until_worker(pid: u32) {
        for _ in 0..400 {
            if deploy_worker::observed_as_worker(pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("the stand-in process {pid} never looked like a worker");
    }

    #[test]
    fn a_stop_or_a_park_in_this_process_keeps_its_lab_busy() {
        // What an update install waits for: a stop runs in this process, never in a worker, and
        // quitting mid-`terraform destroy` leaves billable resources behind.
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

    #[cfg(unix)]
    #[test]
    fn a_running_worker_keeps_its_lab_busy_even_with_an_unreadable_job_file() {
        let t = crate::testing::test_app::TestApp::new();
        let id = "operations-test-worker";
        let dir = t.data().join("deploys");
        std::fs::create_dir_all(&dir).unwrap();
        // A process whose command line reads like a worker's (`… deploy --job …`).
        let mut worker = std::process::Command::new("sh").args(["-c", "sleep 20; :", "deploy", "--job", "x"]).spawn().unwrap();
        // Seen as the worker it stands for only once it has exec'd: wait for that, don't guess.
        wait_until_worker(worker.id());
        std::fs::write(dir.join(format!("{id}.pid")), deploy_worker::pidfile_line(worker.id())).unwrap();
        // Its job file cut short (a full disk, a crash while it was written).
        std::fs::write(dir.join(format!("{id}.json")), "{\"lab_id\":\"operations-test-trunc").unwrap();
        let busy = wait(busy_labs(t.dirs())).unwrap().contains(&id.to_string());
        worker.kill().unwrap();
        worker.wait().unwrap();
        assert!(busy, "an update waits for it");
        assert!(!wait(busy_labs(t.dirs())).unwrap().contains(&id.to_string()), "gone once it ended");
    }

    #[test]
    fn a_stale_pidfile_is_no_operation_and_is_swept() {
        // A worker that died (or finished before its pidfile was removed): not an operation, its
        // pidfile cleared on the way.
        let t = crate::testing::test_app::TestApp::new();
        let id = "operations-test-stale";
        let dir = t.data().join("deploys");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.pid")), "4000000000 1").unwrap();
        std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string(&deploy_worker::Job::on_lab(id, Op::Launch)).unwrap()).unwrap();
        assert!(!wait(busy_labs(t.dirs())).unwrap().contains(&id.to_string()));
        assert!(!dir.join(format!("{id}.pid")).exists(), "swept");
    }

    #[test]
    fn operations_get_their_sidebar_names() {
        assert_eq!(worker_op(&Op::Launch), (Operation::Launch, None));
        assert_eq!(worker_op(&Op::Park { runtime: Runtime::Vm, mode: Park::Pause }).0, Operation::Pause);
        assert_eq!(worker_op(&Op::Park { runtime: Runtime::Vm, mode: Park::Shutdown }).0, Operation::Shutdown);
        assert_eq!(worker_op(&Op::Provision { runtime: Runtime::Vm, machine: Some("dc01".into()) }), (Operation::Provision, Some("dc01".into())));
        // In this process too: a shutdown isn't a pause, a resume or a provisioning run not a launch.
        assert_eq!(Action::Launch.operation(), Operation::Launch);
        assert_eq!(Action::Stop.operation(), Operation::Stop);
        assert_eq!(Action::Park(Park::Pause).operation(), Operation::Pause);
        assert_eq!(Action::Park(Park::Shutdown).operation(), Operation::Shutdown);
        assert_eq!(Action::Resume.operation(), Operation::Resume);
        assert_eq!(Action::Provision.operation(), Operation::Provision);
    }
}
