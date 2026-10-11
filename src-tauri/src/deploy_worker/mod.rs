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
mod process;
mod progress;
mod worker;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

pub use files::{last_deploy_failed, log_so_far};
pub use process::{kill_and_wait, running, running_jobs};
pub use worker::worker_main;

use crate::error::{Error, Result};
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
/// where/how to run it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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

    /// What this operation is, for the "another one is in progress" message.
    fn what(&self) -> &'static str {
        match self.op {
            Op::Launch => "a start",
            Op::Resume { .. } => "a resume",
            Op::Park { .. } => "a pause or shutdown",
            Op::Provision { .. } => "a provisioning run",
            Op::AttackVm { .. } => "an attack VM start",
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
    match spawn_locked(app, &job).await? {
        Ok(spawned) => follow(&spawned, logs).await,
        Err(e) => run_in_app(app, job, logs, e).await,
    }
}

/// Starts the worker under the lab lock, which the expiry reaper holds while it checks for a
/// live worker and tears the lab down: a worker can't start mid-teardown, and one started just
/// before has its pidfile written (in `spawn`) by the time the reaper looks. The outer `Err` is a
/// refusal (busy); the inner one, a worker that couldn't be started.
async fn spawn_locked(app: &AppHandle, job: &Job) -> Result<Result<process::Spawned>> {
    let _lock = crate::runtime::lock_lab(&job.lab_id).await;
    if running(app).contains(&job.key()) {
        return Err(Error::Invalid(format!("{} of this lab is already in progress; wait for it to finish.", capitalize(job.what()))));
    }
    Ok(process::spawn(app, job))
}

/// Follows a started worker's log into `logs` until it finishes.
async fn follow(spawned: &process::Spawned, logs: tauri::ipc::Channel<String>) -> Result<()> {
    progress::tail(spawned, move |line| {
        let _ = logs.send(line);
    })
    .await
}

/// Fallback: runs the job in this process, on a detached task (a webview reload aborts this
/// command future and the child is kill_on_drop, so inline would be cut short). The app then has
/// to stay open until it finishes; the quit guard keeps it alive. The lab lock is not held here:
/// the operation takes it itself.
async fn run_in_app(app: &AppHandle, job: Job, logs: tauri::ipc::Channel<String>, why: Error) -> Result<()> {
    let _ = logs.send(format!("Running inside the app (a background worker couldn't be started: {why}). Keep the app open until it finishes."));
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

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Job, Op, capitalize};

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

    #[test]
    fn the_busy_message_starts_with_a_capital() {
        assert_eq!(capitalize(Job::on_lab("lab-1", Op::AttackVm { box_name: "a/b".into() }).what()), "An attack VM start");
        assert_eq!(capitalize(""), "");
    }
}
