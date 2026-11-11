//! Lab operations that outlive the app. A launch, a resume, a pause or shutdown, and a
//! provisioning run each run in a detached worker process: this same executable,
//! headless (`cyberctf-desktop deploy --job <file>`), in its own process group, with its output
//! in a log file. Quitting, crashing or rebuilding the app never cuts a `vagrant up`, a `compose
//! up` or a `terraform apply` short. The app only follows the worker's log; "in progress" is a
//! live pidfile, so a relaunched app sees the operation and re-attaches to its log.
//!
//! Files, under `<app data>/deploys/`, keyed by the lab: the job, the worker's log and the last
//! run's, its pidfile, its verdict and its lab's last launch verdict (`files::Files` names each).

mod files;
mod process;
#[cfg(all(test, unix))]
pub(crate) use process::{observed_as_worker, pidfile_line};
mod progress;
mod worker;

use crate::AppHandle;
use serde::{Deserialize, Serialize};

pub use files::log_so_far;
pub use process::{kill_and_wait, running, running_jobs};
pub use worker::worker_main;

use crate::error::{Code, Error, Result};
use crate::platform::AppDirs;
use crate::runtime::{Park, Runtime, lab_file_lock};

/// What a worker runs. `Launch` (the default, so job files from before the other kinds existed
/// still read) is a full deploy from a `startLab` payload; the others act on an installed lab.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Launch the lab from its job's `startLab` spec.
    #[default]
    Launch,
    /// Bring a parked (or partly down) lab back as it was.
    Resume {
        /// The lab's runtime.
        runtime: Runtime,
    },
    /// Pause or shut the lab down, machines kept.
    Park {
        /// The lab's runtime.
        runtime: Runtime,
        /// Pause (state saved) or shut down (powered off).
        mode: Park,
    },
    /// Run the provisioners again on one machine (or all).
    Provision {
        /// The lab's runtime.
        runtime: Runtime,
        /// The machine to provision, or none for all of them.
        machine: Option<String>,
    },
    /// Launch a lab imported from a local directory (not the backend): the directory is copied in
    /// and run as a Docker lab with development inputs. `src` is the folder the player picked.
    LaunchLocal {
        /// The folder the player picked.
        src: String,
    },
}

/// What a worker needs: the lab, the operation, and for a launch what to run and where. Its
/// `Debug` leaves the launch's values out ([`crate::labs::Launch`]'s): they carry the launch token.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Job {
    /// The lab the job runs an operation on.
    pub lab_id: String,
    /// Absent in job files from before operations existed: a launch.
    #[serde(default)]
    pub op: Op,
    /// A launch's `startLab` spec and where it runs, beside the operation in the file as job files
    /// have always had them; none for the operations on an installed lab (and for a launch whose
    /// spec can't be read, which then fails).
    #[serde(flatten)]
    pub(crate) launch: Option<LaunchJob>,
}

/// What a launch's job runs: the `startLab` spec, and where and how it runs.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub(crate) struct LaunchJob {
    /// The lab's `startLab` spec and its values.
    pub(crate) launch: crate::labs::Launch,
    /// Where it runs and how (provider, host, ports).
    #[serde(flatten)]
    pub(crate) place: crate::runtime::Placement,
}

impl From<crate::labs::PreparedLaunch> for Job {
    /// A launch's job.
    fn from(l: crate::labs::PreparedLaunch) -> Job {
        let crate::labs::PreparedLaunch { lab_id, launch, place } = l;
        Job { lab_id, op: Op::Launch, launch: Some(LaunchJob { launch, place }) }
    }
}

impl Job {
    /// An operation on an installed lab (no launch payload).
    pub fn on_lab(lab_id: &str, op: Op) -> Job {
        Job { lab_id: lab_id.to_string(), op, launch: None }
    }

    /// The worker files' key: a lab's operations share one (they are serialised by the lab lock
    /// anyway).
    pub fn key(&self) -> String {
        self.lab_id.clone()
    }
}

/// Runs a job in this process (the worker's body, and the in-app fallback when no worker could
/// be started), logging to `log`. A launch returns the lab's local URL.
pub async fn execute(app: &AppHandle, job: Job, log: impl Fn(String)) -> Result<Option<String>> {
    match job.op {
        Op::Launch => {
            let LaunchJob { launch, place } = job.launch.ok_or_else(|| Error::Invalid("invalid launch spec: the job has none".into()))?;
            crate::labs::run(&AppDirs::from(app), launch, place.destination(), log).await
        }
        Op::Resume { runtime } => crate::runtime::resume_lab(&AppDirs::from(app), &job.lab_id, runtime, log).await.map(|()| None),
        Op::Park { runtime, mode } => crate::runtime::park_lab(&AppDirs::from(app), &job.lab_id, runtime, mode, log).await.map(|()| None),
        Op::Provision { runtime, machine } => {
            crate::runtime::provision_lab(&AppDirs::from(app), &job.lab_id, runtime, machine.as_deref(), log).await.map(|()| None)
        }
        Op::LaunchLocal { src } => crate::labs::run_local(&AppDirs::from(app), &job.lab_id, std::path::Path::new(&src), log).await,
    }
}

/// Runs `job` detached (a worker process), following its log into `logs` until it finishes;
/// falls back to running it in this process (on a task that outlives a webview reload) when no
/// worker can be started. Refuses while another operation of the same key is in progress.
pub async fn run_job(app: &AppHandle, job: Job, logs: tauri::ipc::Channel<String>) -> Result<()> {
    match start_worker(app, &job)? {
        Ok(spawned) => {
            let follow = logs.clone();
            progress::tail(&spawned, move |line| {
                let _ = follow.send(line);
            })
            .await
        }
        // Fallback: run in this process, on a detached task (a webview reload aborts this command
        // future and the child is kill_on_drop, so inline would be cut short). The app then has
        // to stay open until it finishes; the quit guard keeps it alive.
        Err(e) => {
            // In the app's log too: the channel is gone with a reload or a closed window, and
            // support reads the log for why a lab ran in the app.
            log::warn!("the deploy worker for {} couldn't start, running it in the app: {e}", job.lab_id);
            let _ = logs.send(format!("Running inside the app (a background worker couldn't be started: {e}). Keep the app open until it finishes."));
            let app = app.clone();
            let handle = tauri::async_runtime::spawn(async move {
                let log = move |line: String| {
                    let _ = logs.send(line);
                };
                execute(&app, job, log).await
            });
            handle.await.map_err(|e| Error::Invalid(format!("the task did not finish: {e}")))??;
            Ok(())
        }
    }
}

/// Held while a worker is checked for and started (see [`start_worker`]).
static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Ends the worker of lab `id`, waiting until it is gone. Called by a stop already marked as
/// stopping (see `runtime::stopping`): a start that passed its checks before the mark has claimed
/// its pidfile once `STARTING` is free, so it is found and ended here.
pub async fn end_workers(dirs: &AppDirs, id: &str) {
    drop(STARTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    kill_and_wait(dirs, id).await;
}

/// Checks that nothing else runs on the job's key and starts its worker, as one step: two
/// starts racing each other (a double click) must not both pass the check and spawn two
/// workers. `Err` when the lab is busy; `Ok(Err)` when the worker couldn't be started (the
/// caller falls back to running the job in this process).
fn start_worker(app: &AppHandle, job: &Job) -> Result<Result<process::Spawned>> {
    let _one_at_a_time = STARTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if running(&AppDirs::from(app)).contains(&job.key()) {
        return Err(Error::coded(Code::AlreadyInProgress));
    }
    // Nothing begins while the lab stops: between ending its workers and taking its lock, a
    // start would win the lock and the stop then destroy what it built.
    if crate::runtime::stopping(&job.lab_id) {
        return Err(Error::Refused("This lab is stopping. Wait for it to stop, then try again.".into()));
    }
    // An operation of the app itself (a stop, the expiry reaper) holds the lab: the worker
    // would only wait for it, then act on what it left.
    if lab_file_lock::held(&lab_file_lock::path(&AppDirs::from(app), &job.lab_id)?) {
        return Err(Error::coded(Code::AlreadyInProgress));
    }
    // The pidfile is taken before the worker starts (another process may be racing for it too).
    let pidfile = process::claim(&files::of(&AppDirs::from(app), &job.key())?)?;
    Ok(process::spawn(app, job, pidfile))
}

/// The files of lab `id`'s worker, for the lab's removal (which holds the lab, so no new worker
/// starts): refused while a worker runs on it, or is being started (its pidfile claimed). Checked
/// as a start checks, so one racing it either claimed its pidfile first or finds the lab held.
pub fn idle_files(dirs: &AppDirs, id: &str) -> Result<Vec<std::path::PathBuf>> {
    let _one_at_a_time = STARTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = files::of(dirs, id)?;
    // `running` clears a stale pidfile: one still there is a live worker's, or one being started.
    if running(dirs).iter().any(|k| k == id) || f.pid.exists() {
        return Err(Error::coded(Code::AlreadyInProgress));
    }
    Ok(f.all())
}

/// Whether the lab's last finished launch failed (see [`files::last_deploy_failed`]).
pub fn last_deploy_failed(dirs: &AppDirs, lab_id: &str) -> bool {
    files::last_deploy_failed(dirs, lab_id)
}

#[cfg(test)]
mod tests {
    use super::{Job, Op};

    /// No worker process could be started (here its job file can't be written; a binary that
    /// can't be run is the same path): the job still runs, inside the app, and the log says so.
    /// What comes back is the job's own outcome, never the failure to start a worker.
    #[tokio::test]
    async fn a_job_runs_inside_the_app_when_no_worker_can_be_started() {
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::rules(&[]);
        let src = std::env::temp_dir().join(format!("cyberctf-notalab-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&src).unwrap();
        let job = Job::on_lab("local-mini", Op::LaunchLocal { src: src.to_string_lossy().into_owned() });
        // A folder where the worker's job file goes: writing the job fails, so no worker starts.
        std::fs::create_dir_all(super::files::of(&t.dirs(), &job.key()).unwrap().job).unwrap();
        let sink = crate::testing::test_app::Sink::default();
        crate::testing::logs::take_warnings();
        let err = super::run_job(t.handle(), job, sink.channel()).await.unwrap_err().to_string();
        let warned = crate::testing::logs::take_warnings();
        assert!(warned.iter().any(|w| w.starts_with("the deploy worker for local-mini couldn't start, running it in the app: ")), "{warned:?}");
        let lines = sink.lines();
        assert!(lines[0].starts_with("Running inside the app (a background worker couldn't be started: "), "{lines:?}");
        assert!(lines[0].ends_with("Keep the app open until it finishes."), "{lines:?}");
        // The job ran here: its own verdict (the folder is no lab), not a failure to start it.
        assert!(err.starts_with("not a lab folder"), "{err}");
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn job_files_from_before_operations_existed_still_read_as_launches() {
        let old = r#"{"lab_id":"lab-1","launch":{"labId":"lab-1","runtime":"VM","repository":"CyberCTF/x","commit":"abc","env":[]},"provider":"virtualbox","host":null,"attackbox_image":null}"#;
        let job: Job = serde_json::from_str(old).unwrap();
        assert_eq!(job.op, Op::Launch);
        let launch = job.launch.as_ref().unwrap();
        assert_eq!((launch.launch.runtime, launch.place.provider), (crate::runtime::Runtime::Vm, Some(crate::runtime::providers::Provider::Virtualbox)));
        assert_eq!(job.key(), "lab-1");
    }

    #[test]
    fn a_job_file_of_the_last_version_reads_with_its_placement() {
        // The layout 1.8 wrote, every field beside the operation, null for a resume: read by the
        // app it was updated to while the worker ran.
        let launch = r#"{"lab_id":"web","op":{"op":"launch"},"launch":{"labId":"web","runtime":"DOCKER","repository":"CyberCTF/web","commit":"abc","env":[{"name":"CTF_LAUNCH_TOKEN","value":"t"}]},"provider":null,"host":"aws1","default_ports":true,"per_machine_vm":false,"docker_context":"orbstack"}"#;
        let job: Job = serde_json::from_str(launch).unwrap();
        let place = &job.launch.as_ref().unwrap().place;
        assert_eq!((place.host.as_deref(), place.default_ports, place.docker_context.as_deref()), (Some("aws1"), true, Some("orbstack")));
        let resume = r#"{"lab_id":"web","op":{"op":"resume","runtime":"VM"},"launch":null,"provider":null,"host":null,"default_ports":false,"per_machine_vm":false,"docker_context":null}"#;
        assert_eq!(serde_json::from_str::<Job>(resume).unwrap(), Job::on_lab("web", Op::Resume { runtime: crate::runtime::Runtime::Vm }));
    }

    #[test]
    fn lab_operations_round_trip_and_share_the_lab_key() {
        for op in [
            Op::Resume { runtime: crate::runtime::Runtime::Vm },
            Op::Park { runtime: crate::runtime::Runtime::Docker, mode: crate::runtime::Park::Shutdown },
            Op::Provision { runtime: crate::runtime::Runtime::Vm, machine: Some("ws01".into()) },
        ] {
            let job = Job::on_lab("lab-1", op);
            let bytes = serde_json::to_vec(&job).unwrap();
            assert_eq!(serde_json::from_slice::<Job>(&bytes).unwrap(), job);
        }
        assert_eq!(Job::on_lab("lab-1", Op::Resume { runtime: crate::runtime::Runtime::Vm }).key(), "lab-1");
    }

    #[test]
    fn a_job_file_with_only_a_lab_id_reads_as_a_plain_launch() {
        // The oldest shape (before provider, host, image or default ports were recorded): every
        // newer field takes its default.
        let job: Job = serde_json::from_str(r#"{"lab_id":"lab-1"}"#).unwrap();
        assert_eq!(job, Job::on_lab("lab-1", Op::Launch));
        assert_eq!(job.key(), "lab-1");
    }

    #[test]
    fn job_files_from_a_newer_version_read_when_only_fields_are_new_not_operations() {
        use crate::runtime::Runtime;
        // A field this version doesn't know (a newer one wrote it): ignored, the job still runs.
        let newer = r#"{"lab_id":"lab-1","op":{"op":"resume","runtime":"VM"},"launch":null,"future_flag":true,"default_ports":true}"#;
        let job: Job = serde_json::from_str(newer).unwrap();
        assert_eq!(job, Job::on_lab("lab-1", Op::Resume { runtime: Runtime::Vm }));
        // An operation this version doesn't know, or one missing what it needs: refused, never
        // run as something else (a launch would download and start the lab).
        for bad in [
            r#"{"lab_id":"lab-1","op":{"op":"reboot"}}"#,
            r#"{"lab_id":"lab-1","op":{"op":"park","runtime":"VM"}}"#,
            r#"{"lab_id":"lab-1","op":{"op":"provision"}}"#,
            r#"{"lab_id":"lab-1","op":{"op":"attack_vm"}}"#,
            r#"{"lab_id":"lab-1","op":{"runtime":"VM"}}"#,
            r#"{"op":{"op":"launch"}}"#,
        ] {
            assert!(serde_json::from_str::<Job>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn job_round_trips_through_its_file_format() {
        let job = Job {
            launch: Some(super::LaunchJob {
                launch: crate::labs::parse_launch(
                    serde_json::json!({ "labId": "lab-1", "runtime": "DOCKER", "repository": "CyberCTF/x", "commit": "abc", "env": [] }),
                )
                .unwrap(),
                place: crate::runtime::Placement {
                    provider: Some(crate::runtime::providers::Provider::Virtualbox),
                    host: None,
                    default_ports: true,
                    per_machine_vm: true,
                    docker_context: Some("orbstack".into()),
                },
            }),
            ..Job::on_lab("lab-1", Op::Launch)
        };
        let bytes = serde_json::to_vec(&job).unwrap();
        assert_eq!(serde_json::from_slice::<Job>(&bytes).unwrap(), job);
    }

    #[tokio::test]
    async fn nothing_starts_on_a_lab_while_it_stops() {
        // Regression: a start between the stop ending the lab's workers and taking its lock won
        // the lock first, and acted on what the stop then destroyed.
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::rules(&[]);
        t.lab("ad", &[]);
        let stopping = crate::runtime::DeployGuard::new("ad", crate::runtime::Action::Stop);
        for op in [Op::Launch, Op::Resume { runtime: crate::runtime::Runtime::Vm }] {
            let job = Job::on_lab("ad", op);
            let err = super::run_job(t.handle(), job.clone(), tauri::ipc::Channel::new(|_| Ok(()))).await.unwrap_err().to_string();
            assert!(err.starts_with("This lab is stopping"), "{err}");
            assert!(!super::files::of(&t.dirs(), &job.key()).unwrap().pid.exists(), "no worker was claimed");
        }
        drop(stopping);
    }

    #[test]
    fn a_job_never_prints_its_launch_token() {
        // A job in a log line (`{job:?}`) must not carry the launch token into the log file.
        let launch = serde_json::json!({ "labId": "lab-1", "runtime": "DOCKER", "repository": "CyberCTF/x", "commit": "abc", "env": [{ "name": "CTF_LAUNCH_TOKEN", "value": "s3cret-token" }] });
        let launch = super::LaunchJob { launch: crate::labs::parse_launch(launch).unwrap(), place: crate::runtime::Placement::default() };
        let job = Job { launch: Some(launch), ..Job::on_lab("lab-1", Op::Launch) };
        let printed = format!("{job:?} {job:#?}");
        assert!(!printed.contains("s3cret-token") && !printed.contains("CTF_LAUNCH_TOKEN"), "{printed}");
        assert!(printed.contains("lab-1") && printed.contains("<redacted>"), "{printed}");
    }
}
