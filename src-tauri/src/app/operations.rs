//! What the UI asks about lab operations in flight, from both places they run: detached deploy
//! workers and this process (a stop, or the fallback when no worker could start). A reloaded or
//! relaunched window rehydrates its "starting" / "stopping" state from these.

use serde::Serialize;

use super::deploy_worker::{self, Op};
use crate::platform::AppDirs;
use crate::runtime::{self, Action, Operation, Runtime};

/// The labs with any operation in flight: every one of this process ([`runtime::busy_labs`]: a
/// start, a provisioning run, a stop, a park), then those a deploy worker runs. Installing an
/// update quits every Cyber CTF process, the workers included, so it waits while this isn't
/// empty: a stop cut off mid-`terraform destroy` leaves billable resources behind as surely as a
/// launch. The same labs as [`active_operations`] lists, without reading each worker's log.
pub async fn busy_labs(dirs: AppDirs) -> Vec<String> {
    or_logged(off_main(move || merge_unique(runtime::busy_labs(), deploy_worker::running(&dirs))).await)
}

/// Runs `f` on a blocking thread. These commands are polled every 2 s by every window, and each
/// look at a worker asks the OS about its process (with retries on macOS) and reads its log: as
/// sync commands they ran on the main thread, stalling every window while they did.
async fn off_main<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// What a poll's blocking task answered; when it panicked, the failure logged and nothing in
/// flight (the windows poll again in a moment).
fn or_logged<T: Default>(answer: Result<T, String>) -> T {
    answer.unwrap_or_else(|e| {
        log::error!("reading the lab operations in flight failed: {e}");
        T::default()
    })
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

/// One operation in flight on a lab, for the sidebar: what it is and the step it is at.
#[derive(Serialize, Debug, PartialEq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActiveOperation {
    lab_id: String,
    /// What it is.
    op: Operation,
    /// The machine a provisioning run targets, when one.
    machine: Option<String>,
    /// Whether the UI shows the lab as deploying: under a deploy worker, whatever its operation,
    /// or brought up in this process (a launch, a resume or a provisioning run). Every operation
    /// listed is in flight ([`busy_labs`]), deploying or not.
    deploying: bool,
    /// The last meaningful line of the operation's log (detached workers only).
    step: Option<String>,
    /// The runtime the lab was launched (or is operated) in, as its worker's job says: what its
    /// status and attack box are read in once the operation ends, catalogue loaded or not.
    /// None for an operation in this process.
    #[ts(optional = nullable)]
    runtime: Option<Runtime>,
}

/// The sidebar's name for a worker's operation (its [`Action`]'s), and the machine it targets
/// when one.
fn worker_op(op: &Op) -> (Operation, Option<String>) {
    match op {
        Op::Launch | Op::LaunchLocal { .. } => (Action::Launch.operation(), None),
        Op::Resume { .. } => (Action::Resume.operation(), None),
        Op::Park { mode, .. } => (Action::Park(*mode).operation(), None),
        Op::Provision { machine, .. } => (Action::Provision.operation(), machine.clone()),
    }
}

/// The runtime a worker's job runs its lab in: a launch's payload names it (`startLab`'s
/// `runtime`), a lab imported from a folder runs as containers, the other operations carry it.
fn job_runtime(job: &deploy_worker::Job) -> Option<Runtime> {
    match &job.op {
        Op::Launch => job.launch.as_ref().map(|l| l.launch.runtime),
        Op::LaunchLocal { .. } => Some(Runtime::Docker),
        Op::Resume { runtime } | Op::Park { runtime, .. } | Op::Provision { runtime, .. } => Some(*runtime),
    }
}

/// Every lab operation in flight: the detached workers (with the step their log is at) and the
/// in-process ones. A lab with a worker is listed once, as the worker's operation.
pub async fn active_operations(dirs: AppDirs) -> Vec<ActiveOperation> {
    or_logged(off_main(move || merge_operations(deploy_worker::running_jobs(&dirs), runtime::active_actions())).await)
}

/// The workers' operations (with their step), then the in-process ones on other labs, by lab id.
fn merge_operations(jobs: Vec<(deploy_worker::Job, Option<String>)>, actions: Vec<(String, Action)>) -> Vec<ActiveOperation> {
    let mut out: Vec<ActiveOperation> = jobs
        .into_iter()
        .map(|(job, step)| {
            let (op, machine) = worker_op(&job.op);
            let runtime = job_runtime(&job);
            ActiveOperation { lab_id: job.lab_id, op, machine, deploying: true, step, runtime }
        })
        .collect();
    for (id, action) in actions {
        if !out.iter().any(|o| o.lab_id == id) {
            out.push(ActiveOperation { lab_id: id, op: action.operation(), machine: None, deploying: action.brings_up(), step: None, runtime: None });
        }
    }
    out.sort_by(|a, b| a.lab_id.cmp(&b.lab_id));
    out
}

/// The log so far of a lab's deploy in its worker process, so a reloaded or relaunched app can
/// re-attach to a deploy still in progress.
///
/// # Errors
///
/// An invalid lab id, a log that can't be read, or a read that couldn't run.
pub async fn lab_deploy_log(dirs: AppDirs, id: String) -> crate::error::Result<String> {
    off_main(move || deploy_worker::log_so_far(&dirs, &id)).await.map_err(crate::error::Error::Invalid)?
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
    fn a_lab_with_a_worker_is_listed_once_as_the_workers_operation() {
        let jobs = vec![
            (deploy_worker::Job::on_lab("web", Op::Launch), Some("Pulling images".to_string())),
            (deploy_worker::Job::on_lab("ad", Op::Provision { runtime: Runtime::Vm, machine: Some("dc01".into()) }), None),
        ];
        let actions = vec![("web".to_string(), Action::Stop), ("ctf".to_string(), Action::Park(Park::Pause))];
        let ops = merge_operations(jobs, actions);
        assert_eq!(
            ops,
            [
                ActiveOperation {
                    lab_id: "ad".into(),
                    op: Operation::Provision,
                    machine: Some("dc01".into()),
                    deploying: true,
                    step: None,
                    runtime: Some(Runtime::Vm)
                },
                ActiveOperation { lab_id: "ctf".into(), op: Operation::Pause, machine: None, deploying: false, step: None, runtime: None },
                ActiveOperation {
                    lab_id: "web".into(),
                    op: Operation::Launch,
                    machine: None,
                    deploying: true,
                    step: Some("Pulling images".into()),
                    runtime: None
                },
            ]
        );
        assert_eq!(merge_operations(Vec::new(), Vec::new()), [] as [ActiveOperation; 0]);
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
        // Serialized for the sidebar in camelCase.
        assert_eq!(serde_json::to_value(&ops[0]).unwrap()["labId"], "ad");
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
    fn an_operation_in_this_process_is_listed_while_it_runs() {
        let t = crate::testing::test_app::TestApp::new();
        // The in-process registry is process-wide: this test's own lab id.
        let id = "operations-test-pause";
        let listed = || wait(active_operations(t.dirs())).into_iter().find(|o| o.lab_id == id);
        let guard = crate::runtime::DeployGuard::new(id, Action::Park(Park::Shutdown));
        assert_eq!(listed(), Some(ActiveOperation { lab_id: id.into(), op: Operation::Shutdown, machine: None, deploying: false, step: None, runtime: None }));
        drop(guard);
        assert_eq!(listed(), None);
        // A resume in this process deploys.
        let guard = crate::runtime::DeployGuard::new(id, Action::Resume);
        assert_eq!(listed().map(|o| (o.op, o.deploying)), Some((Operation::Resume, true)));
        drop(guard);
    }

    #[test]
    fn a_stop_or_a_park_in_this_process_keeps_its_lab_busy() {
        // What an update install waits for: a stop runs in this process, never in a worker, and
        // quitting mid-`terraform destroy` leaves billable resources behind.
        let t = crate::testing::test_app::TestApp::new();
        let busy = |id: &str| wait(busy_labs(t.dirs())).iter().any(|l| l == id);
        for (id, action) in [("operations-test-busy-stop", Action::Stop), ("operations-test-busy-park", Action::Park(Park::Pause))] {
            assert!(!busy(id));
            let guard = crate::runtime::DeployGuard::new(id, action);
            assert!(busy(id), "{action:?} isn't counted");
            // The same labs the sidebar lists as operations in flight.
            assert!(wait(active_operations(t.dirs())).iter().any(|o| o.lab_id == id));
            drop(guard);
            assert!(!busy(id));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_running_worker_is_listed_at_its_step_and_its_log_read_back() {
        let t = crate::testing::test_app::TestApp::new();
        let id = "operations-test-worker";
        let dir = t.data().join("deploys");
        std::fs::create_dir_all(&dir).unwrap();
        // A process whose command line reads like a worker's (`… deploy --job …`).
        let mut worker = std::process::Command::new("sh").args(["-c", "sleep 20; :", "deploy", "--job", "x"]).spawn().unwrap();
        // Seen as the worker it stands for only once it has exec'd: wait for that, don't guess.
        wait_until_worker(worker.id());
        let log = "Pulling images\n==> web: Booting VM...\n";
        std::fs::write(dir.join(format!("{id}.pid")), deploy_worker::pidfile_line(worker.id())).unwrap();
        std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string(&deploy_worker::Job::on_lab(id, Op::Launch)).unwrap()).unwrap();
        std::fs::write(dir.join(format!("{id}.log")), log).unwrap();
        let listed = || wait(active_operations(t.dirs())).into_iter().find(|o| o.lab_id == id);
        assert_eq!(
            listed(),
            Some(ActiveOperation {
                lab_id: id.into(),
                op: Operation::Launch,
                machine: None,
                deploying: true,
                step: Some("web: Booting VM...".into()),
                runtime: None
            })
        );
        assert!(wait(busy_labs(t.dirs())).contains(&id.to_string()));
        assert_eq!(wait(lab_deploy_log(t.dirs(), id.into())).unwrap(), log);
        worker.kill().unwrap();
        worker.wait().unwrap();
        // Gone: no longer listed, its log still readable.
        assert_eq!(listed(), None);
        assert_eq!(wait(lab_deploy_log(t.dirs(), id.into())).unwrap(), log);
    }

    #[test]
    fn a_stale_pidfile_lists_no_operation_and_is_swept() {
        // A worker that died (or finished before its pidfile was removed): not an operation,
        // its pidfile cleared on the way, its log still there to read.
        let t = crate::testing::test_app::TestApp::new();
        let id = "operations-test-stale";
        let dir = t.data().join("deploys");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.pid")), "4000000000 1").unwrap();
        std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string(&deploy_worker::Job::on_lab(id, Op::Launch)).unwrap()).unwrap();
        std::fs::write(dir.join(format!("{id}.log")), "Pulling images\n").unwrap();
        assert!(wait(active_operations(t.dirs())).iter().all(|o| o.lab_id != id));
        assert!(!wait(busy_labs(t.dirs())).contains(&id.to_string()));
        assert!(!dir.join(format!("{id}.pid")).exists(), "swept");
        assert_eq!(wait(lab_deploy_log(t.dirs(), id.into())).unwrap(), "Pulling images\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_live_worker_with_an_unreadable_job_file_stays_in_the_operations_list() {
        let t = crate::testing::test_app::TestApp::new();
        let id = "operations-test-truncated-job";
        let dir = t.data().join("deploys");
        std::fs::create_dir_all(&dir).unwrap();
        let mut worker = std::process::Command::new("sh").args(["-c", "sleep 20; :", "deploy", "--job", "x"]).spawn().unwrap();
        // Seen as the worker it stands for only once it has exec'd: wait for that, don't guess.
        wait_until_worker(worker.id());
        std::fs::write(dir.join(format!("{id}.pid")), deploy_worker::pidfile_line(worker.id())).unwrap();
        // Its job file cut short (a full disk, a crash while it was written).
        std::fs::write(dir.join(format!("{id}.json")), "{\"lab_id\":\"operations-test-trunc").unwrap();
        std::fs::write(dir.join(format!("{id}.log")), "==> web: Booting VM...\n").unwrap();
        let deploying = wait(busy_labs(t.dirs())).contains(&id.to_string());
        let listed = wait(active_operations(t.dirs())).into_iter().find(|o| o.lab_id == id);
        worker.kill().unwrap();
        worker.wait().unwrap();
        assert!(deploying, "the lab page sees the worker");
        // The sidebar should too: at least as a launch at its step.
        assert_eq!(listed.map(|o| (o.op, o.step)), Some((Operation::Launch, Some("web: Booting VM...".to_string()))));
    }

    #[test]
    fn a_deploy_log_is_read_for_a_valid_lab_id_only() {
        let t = crate::testing::test_app::TestApp::new();
        assert_eq!(wait(lab_deploy_log(t.dirs(), "never-deployed".into())).unwrap(), "");
        assert_eq!(wait(lab_deploy_log(t.dirs(), "../x".into())).unwrap_err().to_string(), "invalid lab id `../x`");
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
