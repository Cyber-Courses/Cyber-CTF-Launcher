//! The launcher's own run service: each operation as the launcher ran it before the runner's
//! service, on a task of the app, its log lines and its end told as events. A launch, a resume, a
//! park and a provisioning run go to a detached deploy worker (quitting the app never cuts them
//! short); a stop and the attack box going in or out run in the app.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::{AttachPick, BoxFuture, EventLog, OperationId, Request, RunEvent, RunService};
use crate::app::deploy_worker::{self, Job, Op};
use crate::error::{Code, Error, Result, Wire};
use crate::platform::AppDirs;
use crate::runtime::paths::lab_dir;
use crate::runtime::{Operation, Runtime, attack_box};

/// Where an operation's log lines go.
pub(crate) type Log = Arc<dyn Fn(String) + Send + Sync>;

/// An operation under way.
struct Running {
    /// The lab it acts on.
    lab_id: String,
    /// What it is.
    op: Operation,
    /// Whether it runs in a deploy worker (cancelled by ending the worker).
    in_worker: bool,
    /// Its task, once spawned.
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

/// The launcher's own run service (see the module).
pub(crate) struct LocalRunService {
    /// The app's folders: its data folder holds the store, the labs and the workers' files.
    dirs: AppDirs,
    /// Every operation's events.
    events: Arc<EventLog>,
    /// The operations under way, by id.
    running: Arc<Mutex<HashMap<OperationId, Running>>>,
}

impl LocalRunService {
    /// The service of the app whose folders are `dirs`: no window or app handle needed, so the
    /// command line runs it as the app does.
    pub(crate) fn new(dirs: AppDirs) -> LocalRunService {
        LocalRunService { dirs, events: Arc::new(EventLog::default()), running: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Whether an operation acts on lab `id` now.
    pub(super) fn busy(&self, id: &str) -> bool {
        self.running.lock().unwrap_or_else(PoisonError::into_inner).values().any(|r| r.lab_id == id)
    }

    /// Begins operation `operation` on `lab_id` (told as started), running `work` on a task with
    /// its log; its end is told when `work` returns, or when it is cancelled.
    pub(super) fn begin(&self, started: RunEvent, in_worker: bool, work: impl FnOnce(Log) -> BoxFuture<'static, Result<()>>) {
        let RunEvent::Started { operation, lab_id, op, .. } = &started else { return };
        let operation = operation.clone();
        let (lab_id, op) = (lab_id.clone(), *op);
        self.running.lock().unwrap_or_else(PoisonError::into_inner).insert(operation.clone(), Running { lab_id: lab_id.clone(), op, in_worker, task: None });
        log::info!("operation {operation}: {op:?} of lab {lab_id} started");
        self.events.emit(started);
        let log: Log = {
            let (events, operation, lab_id) = (Arc::clone(&self.events), operation.clone(), lab_id.clone());
            Arc::new(move |line| events.emit(RunEvent::Log { operation: operation.clone(), lab_id: lab_id.clone(), line }))
        };
        let work = work(log);
        let (events, running, id) = (Arc::clone(&self.events), Arc::clone(&self.running), operation.clone());
        let task = tauri::async_runtime::spawn(async move {
            let error = work.await.err().map(|e| e.to_wire());
            end(&events, &running, &id, error);
        });
        if let Some(r) = self.running.lock().unwrap_or_else(PoisonError::into_inner).get_mut(&operation) {
            r.task = Some(task);
        }
    }
}

/// Tells operation `id`'s end (failed with `error`, or done), once: whoever ends it first (the
/// operation itself, or a cancel) tells it.
fn end(events: &EventLog, running: &Mutex<HashMap<OperationId, Running>>, id: &OperationId, error: Option<Wire>) {
    let Some(r) = running.lock().unwrap_or_else(PoisonError::into_inner).remove(id) else { return };
    match &error {
        None => log::info!("operation {id}: {:?} of lab {} done", r.op, r.lab_id),
        Some(e) => log::warn!("operation {id}: {:?} of lab {} failed: {}", r.op, r.lab_id, e.message),
    }
    events.emit(RunEvent::Ended { operation: id.clone(), lab_id: r.lab_id, op: r.op, error });
}

/// Now, in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl RunService for LocalRunService {
    fn start(&self, request: Request) -> Result<OperationId> {
        let lab_id = request.lab_id().to_owned();
        crate::runtime::validate_id(&lab_id)?;
        if !matches!(request, Request::Stop { .. }) && self.busy(&lab_id) {
            return Err(Error::coded(Code::AlreadyInProgress));
        }
        let operation = OperationId::new();
        let started = RunEvent::Started {
            operation: operation.clone(),
            lab_id,
            op: request.operation(),
            machine: request.machine(),
            runtime: request.runtime(),
            deploying: request.deploying(),
            at: now_ms(),
        };
        let in_worker = !matches!(request, Request::Stop { .. } | Request::AttachBox { .. } | Request::DetachBox { .. });
        let dirs = self.dirs.clone();
        self.begin(started, in_worker, move |log| Box::pin(async move { execute(&dirs, request, log).await }));
        Ok(operation)
    }

    fn cancel(&self, operation: &OperationId) -> BoxFuture<'_, ()> {
        let operation = operation.clone();
        Box::pin(async move {
            let found = self.running.lock().unwrap_or_else(PoisonError::into_inner).get_mut(&operation).map(|r| (r.lab_id.clone(), r.in_worker, r.task.take()));
            let Some((lab_id, in_worker, task)) = found else { return };
            if in_worker {
                deploy_worker::kill_and_wait(&self.dirs, &lab_id).await;
            }
            if let Some(task) = task {
                task.abort();
            }
            let cancelled = Error::Invalid("the operation was cancelled before it ended".into());
            end(&self.events, &self.running, &operation, Some(cancelled.to_wire()));
        })
    }

    fn events(&self) -> &EventLog {
        &self.events
    }

    fn pick_up_running(&self) {
        let dirs = self.dirs.clone();
        for (job, _step) in deploy_worker::running_jobs(&dirs) {
            if self.busy(&job.lab_id) {
                continue;
            }
            let (op, machine) = crate::app::operations::worker_op(&job.op);
            let operation = OperationId::new();
            let started = RunEvent::Started {
                operation: operation.clone(),
                lab_id: job.lab_id.clone(),
                op,
                machine,
                runtime: crate::app::operations::job_runtime(&job),
                deploying: true,
                at: now_ms(),
            };
            let (dirs, id) = (dirs.clone(), job.lab_id.clone());
            self.begin(started, true, move |log| Box::pin(async move { deploy_worker::follow(&dirs, &id, move |line| log(line)).await }));
        }
    }
}

/// Runs `request` in the app whose folders are `dirs`, its log lines to `log`.
async fn execute(dirs: &AppDirs, request: Request, log: Log) -> Result<()> {
    match request {
        Request::Launch(launch) => {
            let line = |l: String| log(l);
            let prepared = crate::labs::prepare_launch(dirs, launch, &line).await?;
            deploy_worker::run_job(dirs, Job::from(prepared), log).await
        }
        Request::LaunchLocal { lab_id, src } => {
            deploy_worker::run_job(dirs, Job::on_lab(&lab_id, Op::LaunchLocal { src: src.to_string_lossy().into_owned() }), log).await
        }
        Request::Resume { lab_id, runtime } => worker(dirs, &lab_id, Op::Resume { runtime }, log).await,
        Request::Park { lab_id, runtime, mode } => worker(dirs, &lab_id, Op::Park { runtime, mode }, log).await,
        Request::Provision { lab_id, runtime, machine } => worker(dirs, &lab_id, Op::Provision { runtime, machine }, log).await,
        Request::Stop { lab_id, runtime } => {
            crate::labs::stop_lab_retiring_colocation(dirs, &lab_id, runtime, deploy_worker::end_workers(dirs, &lab_id), move |l| log(l)).await
        }
        Request::AttachBox { lab_id, runtime, pick } => Box::pin(attach_box(dirs, &lab_id, runtime, pick, log)).await,
        Request::DetachBox { lab_id, runtime } => Box::pin(detach_box(dirs, &lab_id, runtime, log)).await,
    }
}

/// Runs operation `op` on installed lab `id` in a deploy worker.
async fn worker(dirs: &AppDirs, id: &str, op: Op, log: Log) -> Result<()> {
    lab_dir(dirs, id)?;
    deploy_worker::run_job(dirs, Job::on_lab(id, op), log).await
}

/// Adds the attack box to running lab `id` as a machine of it, on the network the learner picked
/// (else the seat the lab declares, else its first network), then brings the run up to date; a
/// container box pulls its image on the first add.
async fn attach_box(dirs: &AppDirs, id: &str, runtime: Runtime, pick: AttachPick, log: Log) -> Result<()> {
    let AttachPick { network, image, desktop } = pick;
    let dir = lab_dir(dirs, id)?;
    if let Some(image) = &image
        && !crate::platform::exec::valid_image(image)
    {
        return Err(Error::Invalid(format!("invalid attack-box image `{image}`")));
    }
    // Held for the whole attach, like every lab operation: a pause or a stop from another window
    // or the agent waits, rather than halting the VMs the attach is working in.
    let _lock = crate::runtime::lock_lab(dirs, id).await?;
    let run = crate::runtime::attack_box::reachable(dirs, &dir, id, runtime)?;
    let mut line = move |l: String| log(l);
    let image = image.unwrap_or_else(|| attack_box::default_image().to_owned());
    // In flight like every lab operation: nothing that would collide begins meanwhile, while
    // bringing the run up to date may restart some of its machines.
    crate::runtime::as_operation(id, crate::runtime::Action::AttachBox, async {
        let attach = run.add(dirs, attack_box::Pick { image: &image, desktop }, network.as_deref(), &mut line);
        // Only an image pulled on this machine is the app's to account for.
        if !run.pulls_image_here() {
            return Box::pin(attach).await;
        }
        // Boxed: the runner's attach future is large, and tracking it adds to it.
        Box::pin(crate::machine::storage::owned::tracked(dirs.clone(), std::slice::from_ref(&image), &[], attach)).await
    })
    .await
}

/// Takes the attack box back out of running lab `id`, leaving the rest of the lab up.
async fn detach_box(dirs: &AppDirs, id: &str, runtime: Runtime, log: Log) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    let _lock = crate::runtime::lock_lab(dirs, id).await?;
    let run = crate::runtime::attack_box::reachable(dirs, &dir, id, runtime)?;
    let mut line = move |l: String| log(l);
    crate::runtime::as_operation(id, crate::runtime::Action::DetachBox, Box::pin(run.remove(dirs, &mut line))).await
}
