//! Lab operations that outlive the app. A launch, a resume, a pause or shutdown, a provisioning
//! run and an attack VM start each run in a detached worker process: this same executable,
//! headless (`cyberctf-desktop deploy --job <file>`), in its own process group, with its output
//! in a log file. Quitting, crashing or rebuilding the app never cuts a `vagrant up`, a `compose
//! up` or a `terraform apply` short. The app only follows the worker's log; "in progress" is a
//! live pidfile, so a relaunched app sees the operation and re-attaches to its log.
//!
//! Files, under `<app data>/deploys/`, keyed by the lab (`<lab>`) or, for its attack VM, which
//! runs beside the lab's own operation, `<lab>.attack`: `<key>.json` (the job), `<key>.log`
//! (what the worker says, also its stdout/stderr), `<key>.pid` (the worker, while it runs),
//! `<key>.status` (`ok` or `error: ...`, written last).

mod files;
pub(crate) mod lock;
mod process;
#[cfg(all(test, unix))]
pub(crate) use process::pidfile_line;
mod progress;
mod worker;

use crate::AppHandle;
use serde::{Deserialize, Serialize};

pub use files::{last_deploy_failed, log_so_far};
pub use process::{kill_and_wait, running, running_jobs};
pub use worker::worker_main;

use crate::error::{Code, Error, Result};
use crate::runtime::providers::Provider;
use crate::runtime::{Park, Runtime};

/// What a worker runs. `Launch` (the default, so job files from before the other kinds existed
/// still read) is a full deploy from a `startLab` payload; the others act on an installed lab.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    #[default]
    Launch,
    /// Bring a parked (or partly down) lab back as it was.
    Resume { runtime: Runtime },
    /// Pause or shut the lab down, machines kept.
    Park { runtime: Runtime, mode: Park },
    /// Run the provisioners again on one machine (or all).
    Provision { runtime: Runtime, machine: Option<String> },
    /// Start the attack VM beside a VM lab.
    AttackVm { box_name: String },
}

/// What a worker needs: the lab, the operation, and for a launch the `startLab` payload and
/// where/how to run it. Its `Debug` leaves the payload out: it carries the launch token.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Job {
    pub lab_id: String,
    /// Absent in job files from before operations existed: a launch.
    #[serde(default)]
    pub op: Op,
    /// The `startLab` payload (`{ labId, runtime, repository, commit, env }`); null for the
    /// operations on an installed lab.
    #[serde(default)]
    pub launch: serde_json::Value,
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub attackbox_image: Option<String>,
    /// A container lab on the lab's own ports rather than random free ones.
    #[serde(default)]
    pub default_ports: bool,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("lab_id", &self.lab_id)
            .field("op", &self.op)
            .field("launch", &if self.launch.is_null() { "none" } else { "<redacted>" })
            .field("provider", &self.provider)
            .field("host", &self.host)
            .field("attackbox_image", &self.attackbox_image)
            .field("default_ports", &self.default_ports)
            .finish()
    }
}

impl Job {
    /// An operation on an installed lab (no launch payload).
    pub fn on_lab(lab_id: &str, op: Op) -> Job {
        Job { lab_id: lab_id.to_string(), op, launch: serde_json::Value::Null, provider: None, host: None, attackbox_image: None, default_ports: false }
    }

    /// The worker files' key: the lab's own operations share one (they are serialised by the
    /// lab lock anyway); the attack VM runs beside them under its own.
    pub fn key(&self) -> String {
        match self.op {
            Op::AttackVm { .. } => attack_key(&self.lab_id),
            _ => self.lab_id.clone(),
        }
    }
}

/// The worker key of a lab's attack VM operation.
pub fn attack_key(lab_id: &str) -> String {
    format!("{lab_id}.attack")
}

/// Runs a job in this process (the worker's body, and the in-app fallback when no worker could
/// be started), logging to `log`. A launch returns the lab's local URL.
pub async fn execute(app: &AppHandle, job: Job, log: impl Fn(String)) -> Result<Option<String>> {
    match job.op {
        Op::Launch => crate::labs::run(app, job.launch, job.provider, job.host.as_deref(), job.attackbox_image.as_deref(), job.default_ports, log).await,
        Op::Resume { runtime } => crate::runtime::resume_lab(app, &job.lab_id, runtime, log).await.map(|_| None),
        Op::Park { runtime, mode } => crate::runtime::park_lab(app, &job.lab_id, runtime, mode, log).await.map(|_| None),
        Op::Provision { runtime, machine } => crate::runtime::provision_lab(app, &job.lab_id, runtime, machine.as_deref(), log).await.map(|_| None),
        Op::AttackVm { box_name } => crate::runtime::start_attack_vm(app, &job.lab_id, &box_name, log).await.map(|_| None),
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

/// Ends the workers of lab `id` (its own operation's and its attack VM's), waiting until they
/// are gone. Called by a stop already marked as stopping (see `runtime::stopping`): a start that
/// passed its checks before the mark has claimed its pidfile once `STARTING` is free, so it is
/// found and ended here, and every later attack VM start is refused.
pub async fn end_workers(app: &AppHandle, id: &str) {
    drop(STARTING.lock().unwrap_or_else(|e| e.into_inner()));
    kill_and_wait(app, id).await;
    kill_and_wait(app, &attack_key(id)).await;
}

/// Checks that nothing else runs on the job's key and starts its worker, as one step: two
/// starts racing each other (a double click) must not both pass the check and spawn two
/// workers. `Err` when the lab is busy; `Ok(Err)` when the worker couldn't be started (the
/// caller falls back to running the job in this process).
fn start_worker(app: &AppHandle, job: &Job) -> Result<Result<process::Spawned>> {
    let _one_at_a_time = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if running(app).contains(&job.key()) {
        return Err(Error::coded(Code::AlreadyInProgress));
    }
    // Nothing begins while the lab stops: between ending its workers and taking its lock, a
    // start would win the lock and the stop then destroy what it built; and the attack VM takes
    // no lab lock (it boots beside a lab that is still starting), so it would come up orphaned.
    if crate::runtime::stopping(&job.lab_id) {
        return Err(Error::Refused("This lab is stopping. Wait for it to stop, then try again.".into()));
    }
    // An operation of the app itself (a stop, the expiry reaper) holds the lab: the worker
    // would only wait for it, then act on what it left (the attack VM runs beside the lab's
    // operations and takes no lab lock).
    if !matches!(job.op, Op::AttackVm { .. }) && lock::held(&lock_path(app, &job.lab_id)?) {
        return Err(Error::coded(Code::AlreadyInProgress));
    }
    // The pidfile is taken before the worker starts (another process may be racing for it too).
    let pidfile = process::claim(&files::of(app, &job.key())?)?;
    Ok(process::spawn(app, job, pidfile))
}

/// The cross-process lock file of `lab` (see [`lock`]).
pub fn lock_path(app: &AppHandle, lab: &str) -> Result<std::path::PathBuf> {
    crate::runtime::validate_id(lab)?;
    Ok(lock::path_in(&files::dir(app)?, lab))
}

#[cfg(test)]
mod tests {
    use super::{Job, Op};

    #[test]
    fn job_files_from_before_operations_existed_still_read_as_launches() {
        let old = r#"{"lab_id":"lab-1","launch":{"labId":"lab-1"},"provider":null,"host":null,"attackbox_image":null}"#;
        let job: Job = serde_json::from_str(old).unwrap();
        assert_eq!(job.op, Op::Launch);
        assert_eq!(job.key(), "lab-1");
    }

    #[test]
    fn lab_operations_round_trip_and_the_attack_vm_keeps_its_own_key() {
        for op in [
            Op::Resume { runtime: crate::runtime::Runtime::Vm },
            Op::Park { runtime: crate::runtime::Runtime::Docker, mode: crate::runtime::Park::Shutdown },
            Op::Provision { runtime: crate::runtime::Runtime::Vm, machine: Some("ws01".into()) },
            Op::AttackVm { box_name: "kalilinux/rolling".into() },
        ] {
            let job = Job::on_lab("lab-1", op);
            let bytes = serde_json::to_vec(&job).unwrap();
            assert_eq!(serde_json::from_slice::<Job>(&bytes).unwrap(), job);
        }
        assert_eq!(Job::on_lab("lab-1", Op::AttackVm { box_name: "a/b".into() }).key(), "lab-1.attack");
        assert_eq!(Job::on_lab("lab-1", Op::Resume { runtime: crate::runtime::Runtime::Vm }).key(), "lab-1");
    }

    #[test]
    fn a_job_file_with_only_a_lab_id_reads_as_a_plain_launch() {
        // The oldest shape (before provider, host, image or default ports were recorded): every
        // newer field takes its default.
        let job: Job = serde_json::from_str(r#"{"lab_id":"lab-1"}"#).unwrap();
        assert_eq!(job, Job::on_lab("lab-1", Op::Launch));
        assert!(job.launch.is_null() && !job.default_ports && job.provider.is_none() && job.host.is_none() && job.attackbox_image.is_none());
        assert_eq!(job.key(), "lab-1");
    }

    #[test]
    fn job_files_from_a_newer_version_read_when_only_fields_are_new_not_operations() {
        use crate::runtime::Runtime;
        // A field this version doesn't know (a newer one wrote it): ignored, the job still runs.
        let newer = r#"{"lab_id":"lab-1","op":{"op":"resume","runtime":"VM"},"launch":null,"future_flag":true,"default_ports":true}"#;
        let job: Job = serde_json::from_str(newer).unwrap();
        assert_eq!(job.op, Op::Resume { runtime: Runtime::Vm });
        assert!(job.default_ports);
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
            lab_id: "lab-1".into(),
            op: Op::Launch,
            launch: serde_json::json!({ "labId": "lab-1", "runtime": "DOCKER", "repository": "CyberCTF/x", "commit": "abc", "env": [] }),
            provider: Some(crate::runtime::providers::Provider::Virtualbox),
            host: None,
            attackbox_image: Some("cyberctf/attack-box".into()),
            default_ports: true,
        };
        let bytes = serde_json::to_vec(&job).unwrap();
        assert_eq!(serde_json::from_slice::<Job>(&bytes).unwrap(), job);
    }

    #[tokio::test]
    async fn nothing_starts_on_a_lab_while_it_stops() {
        // Regression: an attack VM start during a Stop passed its "lab is running" check early in
        // the destroy, and its VM came up after the lab was gone; a start between the stop ending
        // the lab's workers and taking its lock won the lock first.
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::rules(&[]);
        t.lab("ad", &[]);
        let stopping = crate::runtime::DeployGuard::new("ad", crate::runtime::Action::Stop);
        for op in [Op::AttackVm { box_name: "kalilinux/rolling".into() }, Op::Launch, Op::Resume { runtime: crate::runtime::Runtime::Vm }] {
            let job = Job::on_lab("ad", op);
            let err = super::run_job(t.handle(), job.clone(), tauri::ipc::Channel::new(|_| Ok(()))).await.unwrap_err().to_string();
            assert!(err.starts_with("This lab is stopping"), "{err}");
            assert!(!super::files::of(t.handle(), &job.key()).unwrap().pid.exists(), "no worker was claimed");
        }
        drop(stopping);
    }

    #[test]
    fn a_job_never_prints_its_launch_token() {
        // A job in a log line (`{job:?}`) must not carry the launch token into the log file.
        let mut job = Job::on_lab("lab-1", Op::Launch);
        job.launch = serde_json::json!({ "labId": "lab-1", "env": [{ "name": "CTF_LAUNCH_TOKEN", "value": "s3cret-token" }] });
        let printed = format!("{job:?} {job:#?}");
        assert!(!printed.contains("s3cret-token") && !printed.contains("CTF_LAUNCH_TOKEN"), "{printed}");
        assert!(printed.contains("lab-1") && printed.contains("<redacted>"), "{printed}");
    }
}
