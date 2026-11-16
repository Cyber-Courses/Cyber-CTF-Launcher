//! The launcher's own run service: each operation on a task of the app, its log lines and its end
//! told as events. What it asks of a lab's run (its `up`, `down`, `park`…) goes to the home's
//! runner service, which runs it to its end whatever becomes of the app; the launcher's own steps
//! around it (the download and install, the place, the attack box's edits) run here. The website
//! agent's launch and teardown, which run in the app on their own, are told the same way
//! ([`LocalRunService::observe`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::watch::{self, Watch};
use super::{AttachPick, BoxFuture, EventLog, OperationId, Request, RunEvent, RunService};
use crate::error::{Code, Error, Result, Wire};
use crate::platform::AppDirs;
use crate::runtime::isothrum::Others;
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
}

/// The launcher's own run service (see the module).
pub(crate) struct LocalRunService {
    /// The app's folders: its data folder holds the store, the labs and the workers' files.
    dirs: AppDirs,
    /// Every operation's events.
    events: Arc<EventLog>,
    /// The operations under way, by id.
    running: Arc<Mutex<HashMap<OperationId, Running>>>,
    /// What is watched, and what was told of it.
    watch: Arc<Mutex<Watch>>,
}

/// The services of this process, one per data folder: the app's windows, the website agent and
/// a command line in this process all report through the same one.
fn services() -> &'static Mutex<HashMap<std::path::PathBuf, Arc<LocalRunService>>> {
    static SERVICES: std::sync::OnceLock<Mutex<HashMap<std::path::PathBuf, Arc<LocalRunService>>>> = std::sync::OnceLock::new();
    SERVICES.get_or_init(|| Mutex::new(HashMap::new()))
}

impl LocalRunService {
    /// The service of the app whose folders are `dirs`: no window or app handle needed, so the
    /// command line runs it as the app does.
    pub(crate) fn new(dirs: AppDirs) -> LocalRunService {
        LocalRunService {
            dirs,
            events: Arc::new(EventLog::default()),
            running: Arc::new(Mutex::new(HashMap::new())),
            watch: Arc::new(Mutex::new(Watch::default())),
        }
    }

    /// This process's service for the data folder of `dirs`, made on first use.
    ///
    /// # Errors
    ///
    /// No data folder.
    pub(crate) fn of(dirs: &AppDirs) -> Result<Arc<LocalRunService>> {
        let data = dirs.data()?.to_path_buf();
        let mut all = services().lock().unwrap_or_else(PoisonError::into_inner);
        Ok(Arc::clone(all.entry(data).or_insert_with(|| Arc::new(LocalRunService::new(dirs.clone())))))
    }

    /// Whether an operation acts on lab `id` now.
    pub(super) fn is_busy(&self, id: &str) -> bool {
        self.running.lock().unwrap_or_else(PoisonError::into_inner).values().any(|r| r.lab_id == id)
    }

    /// The labs with an operation of any kind under way in this process (a start, a resume, a
    /// provisioning run, a stop, a park, the attack box), each once: what a quit would cut off
    /// (the launcher's own steps; the run's operations go on in the runner service).
    pub(crate) fn busy_labs(&self) -> Vec<String> {
        let running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        let mut labs: Vec<String> = running.values().map(|r| r.lab_id.clone()).collect();
        labs.sort();
        labs.dedup();
        labs
    }

    /// Marks lab `id` as under operation `op` until the hold drops (a test's stand-in for an
    /// operation under way).
    #[cfg(test)]
    pub(crate) fn hold(self: &Arc<Self>, id: &str, op: Operation) -> impl Drop {
        struct Hold(Arc<LocalRunService>, OperationId);
        impl Drop for Hold {
            fn drop(&mut self) {
                self.0.running.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.1);
            }
        }
        let operation = OperationId::new();
        self.running.lock().unwrap_or_else(PoisonError::into_inner).insert(operation.clone(), Running { lab_id: id.to_owned(), op });
        Hold(Arc::clone(self), operation)
    }

    /// Whether lab `id` is being stopped in this process now.
    pub(crate) fn stopping(&self, id: &str) -> bool {
        self.running.lock().unwrap_or_else(PoisonError::into_inner).values().any(|r| r.lab_id == id && r.op == Operation::Stop)
    }

    /// Admits operation `operation`, `op` on `lab_id`, as under way: refused when `exclusive`
    /// and another acts on the lab. Checked and recorded as one step: two starts racing each
    /// other (a double click) never both pass.
    fn admit(&self, operation: &OperationId, lab_id: &str, op: Operation, exclusive: bool) -> Result<()> {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if exclusive && running.values().any(|r| r.lab_id == lab_id) {
            return Err(Error::coded(Code::AlreadyInProgress));
        }
        running.insert(operation.clone(), Running { lab_id: lab_id.to_owned(), op });
        Ok(())
    }

    /// The log of operation `operation` on `lab_id`: each line an event.
    fn log_of(&self, operation: &OperationId, lab_id: &str) -> Log {
        let (events, operation, lab_id) = (Arc::clone(&self.events), operation.clone(), lab_id.to_owned());
        Arc::new(move |line| events.emit(RunEvent::Log { operation: operation.clone(), lab_id: lab_id.clone(), line }))
    }

    /// Begins the operation `started` tells of (refused, nothing told, when `exclusive` and
    /// another acts on its lab), running `work` on a task with its log; its end is told when
    /// `work` returns, or panics.
    pub(super) fn begin(&self, started: RunEvent, exclusive: bool, work: impl FnOnce(Log) -> BoxFuture<'static, Result<()>>) -> Result<()> {
        let RunEvent::Started { operation, lab_id, op, .. } = &started else { return Ok(()) };
        let (operation, lab_id, op) = (operation.clone(), lab_id.clone(), *op);
        self.admit(&operation, &lab_id, op, exclusive)?;
        log::info!("operation {operation}: {op:?} of lab {lab_id} started");
        self.events.emit(started);
        let work = work(self.log_of(&operation, &lab_id));
        let (events, running) = (Arc::clone(&self.events), Arc::clone(&self.running));
        crate::spawn(async move {
            let error = unpanicked(work).await.err().map(|e| e.to_wire());
            end(&events, &running, &operation, error);
        });
        Ok(())
    }

    /// Runs `work`, operation `op` on lab `id` started outside a request (the website agent's
    /// launch and teardown), here and now, told as an operation like the others: its start, its
    /// log lines, its end. Never refused: what it does takes the lab's lock itself.
    ///
    /// # Errors
    ///
    /// What `work` returns, or its panic as an error.
    pub(crate) async fn observe<T, F>(&self, id: &str, op: Operation, runtime: Option<Runtime>, work: impl FnOnce(Log) -> F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let operation = OperationId::new();
        self.admit(&operation, id, op, false)?;
        log::info!("operation {operation}: {op:?} of lab {id} started");
        let deploying = !matches!(op, Operation::Stop | Operation::Pause | Operation::Shutdown);
        self.events.emit(RunEvent::Started { operation: operation.clone(), lab_id: id.to_owned(), op, machine: None, runtime, deploying, at: now_ms() });
        let out = unpanicked(work(self.log_of(&operation, id))).await;
        end(&self.events, &self.running, &operation, out.as_ref().err().map(Error::to_wire));
        out
    }
}

/// `work`'s result, a panic in it as an error: an operation that panics still ends (its lab no
/// longer busy, its page no longer waiting).
async fn unpanicked<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    crate::platform::unwind::CatchUnwind(Box::pin(work))
        .await
        .unwrap_or_else(|panic| Err(Error::Invalid(format!("the operation stopped on an internal error: {}", crate::platform::unwind::panic_message(&*panic)))))
}

/// Reads lab `id`, declared `runtime`, and tells its status when it says something new.
async fn tell_status(dirs: &AppDirs, events: &EventLog, watched: &Mutex<Watch>, id: String, runtime: Runtime) {
    let read = watch::read(dirs, &id, runtime).await;
    if !watched.lock().unwrap_or_else(PoisonError::into_inner).told(&id, &read) {
        return;
    }
    let (status, error) = match read {
        Ok(status) => (Some(Box::new(status)), None),
        Err(error) => (None, Some(error)),
    };
    events.emit(RunEvent::Status { lab_id: id, status, error });
}

/// Tells operation `id`'s end (failed with `error`, or done), once.
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
        // A stop is never refused: it ends what runs on the lab first.
        let exclusive = !matches!(request, Request::Stop { .. });
        let dirs = self.dirs.clone();
        self.begin(started, exclusive, move |log| Box::pin(async move { execute(&dirs, request, log).await }))?;
        Ok(operation)
    }

    fn events(&self) -> &EventLog {
        &self.events
    }

    fn watch(&self, labs: Vec<(String, Runtime)>, open: Vec<String>) {
        let fresh = self.watch.lock().unwrap_or_else(PoisonError::into_inner).set(labs, open);
        if fresh.is_empty() {
            return;
        }
        let (dirs, events, watched) = (self.dirs.clone(), Arc::clone(&self.events), Arc::clone(&self.watch));
        crate::spawn(async move {
            for (id, runtime) in fresh {
                tell_status(&dirs, &events, &watched, id, runtime).await;
            }
        });
    }

    fn reconcile_once(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let seen = crate::runtime::recorded::running_here(&self.dirs).await.into_iter().collect::<std::collections::BTreeSet<_>>();
            let lab_ids: Vec<String> = seen.iter().cloned().collect();
            if self.watch.lock().unwrap_or_else(PoisonError::into_inner).scanned(seen) {
                self.events.emit(RunEvent::Running { lab_ids });
            }
            let picked = self.watch.lock().unwrap_or_else(PoisonError::into_inner).next_reads(|id| self.is_busy(id));
            for (id, runtime) in picked {
                tell_status(&self.dirs, &self.events, &self.watch, id, runtime).await;
            }
        })
    }

    fn follow_runner(&self) -> BoxFuture<'static, ()> {
        let (dirs, events, running) = (self.dirs.clone(), Arc::clone(&self.events), Arc::clone(&self.running));
        Box::pin(async move {
            let mut told: HashMap<i64, (OperationId, String)> = HashMap::new();
            let followed = crate::runtime::isothrum::follow_others(&dirs, &mut |seen| tell(&events, &running, &mut told, seen)).await;
            if let Err(e) = followed {
                log::debug!("the runner service isn't followed now: {e}");
            }
            // Told begun and not ended: their end is the service's to tell, which the next follow
            // can't (it tells only what happens from then on).
            let lost = || Error::Invalid("the runner service's stream ended before the operation did: see `cyberctf labs logs`".into()).to_wire();
            for (id, _) in told.into_values() {
                end(&events, &running, &id, Some(lost()));
            }
        })
    }
}

/// Tells what the runner service did that this process didn't ask for (`seen`), as run events:
/// another process's operation (`told`: those begun, by the service's id), and a run that left
/// its standing state, whose lab's screens end.
fn tell(events: &EventLog, running: &Mutex<HashMap<OperationId, Running>>, told: &mut HashMap<i64, (OperationId, String)>, seen: Others) {
    let lab_of = |run: &str| crate::runtime::lab_of_project(run).unwrap_or(run).to_owned();
    match seen {
        Others::Began { operation, run, op } => {
            let Some(op) = ui_op(op) else { return };
            let (id, lab_id) = (OperationId(format!("runner-{operation}")), lab_of(&run));
            running.lock().unwrap_or_else(PoisonError::into_inner).insert(id.clone(), Running { lab_id: lab_id.clone(), op });
            let deploying = !matches!(op, Operation::Stop | Operation::Pause | Operation::Shutdown);
            events.emit(RunEvent::Started { operation: id.clone(), lab_id: lab_id.clone(), op, machine: None, runtime: None, deploying, at: now_ms() });
            told.insert(operation, (id, lab_id));
        }
        Others::Line { operation, line } => {
            if let Some((id, lab_id)) = told.get(&operation) {
                events.emit(RunEvent::Log { operation: id.clone(), lab_id: lab_id.clone(), line });
            }
        }
        Others::Ended { operation, error } => {
            if let Some((id, _)) = told.remove(&operation) {
                end(events, running, &id, error.map(|e| e.to_wire()));
            }
        }
        Others::Left { run } => crate::screen::close_lab(&lab_of(&run)),
    }
}

/// The UI's name of the runner's operation `op`; none for one the UI doesn't show (a check, an
/// update of the run's project).
fn ui_op(op: isothrum_runner::Op) -> Option<Operation> {
    use isothrum_runner::Op;
    Some(match op {
        Op::Up => Operation::Launch,
        Op::Down => Operation::Stop,
        Op::Park => Operation::Pause,
        Op::Resume => Operation::Resume,
        Op::Provision => Operation::Provision,
        Op::Attach => Operation::Attach,
        Op::Detach | Op::Retire => Operation::Detach,
        _ => return None,
    })
}

/// Runs `request` in the app whose folders are `dirs`, its log lines to `log`.
async fn execute(dirs: &AppDirs, request: Request, log: Log) -> Result<()> {
    match request {
        Request::Launch(launch) => {
            let line = |l: String| log(l);
            let crate::labs::PreparedLaunch { launch, place, .. } = crate::labs::prepare_launch(dirs, launch, &line).await?;
            Box::pin(crate::labs::run(dirs, launch, place.destination(), line)).await.map(drop)
        }
        Request::LaunchLocal { lab_id, src } => Box::pin(crate::labs::run_local(dirs, &lab_id, &src, move |l| log(l))).await.map(drop),
        Request::Resume { lab_id, runtime } => Box::pin(crate::runtime::resume_lab(dirs, &lab_id, runtime, move |l| log(l))).await,
        Request::Park { lab_id, runtime, mode } => Box::pin(crate::runtime::park_lab(dirs, &lab_id, runtime, mode, move |l| log(l))).await,
        Request::Provision { lab_id, runtime, machine } => {
            Box::pin(crate::runtime::provision_lab(dirs, &lab_id, runtime, machine.as_deref(), move |l| log(l))).await
        }
        Request::Stop { lab_id, runtime } => {
            let ended = crate::runtime::isothrum::end_operations(dirs, &lab_id);
            crate::labs::stop_lab_retiring_colocation(dirs, &lab_id, runtime, ended, move |l| log(l)).await
        }
        Request::AttachBox { lab_id, runtime, pick } => Box::pin(attach_box(dirs, &lab_id, runtime, pick, log)).await,
        Request::DetachBox { lab_id, runtime } => Box::pin(detach_box(dirs, &lab_id, runtime, log)).await,
    }
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
    let attach = run.add(dirs, attack_box::Pick { image: &image, desktop }, network.as_deref(), &mut line);
    // Only an image pulled on this machine is the app's to account for.
    if !run.pulls_image_here() {
        return Box::pin(attach).await;
    }
    // Boxed: the runner's attach future is large, and tracking it adds to it.
    Box::pin(crate::machine::storage::owned::tracked(dirs.clone(), std::slice::from_ref(&image), &[], attach)).await
}

/// Takes the attack box back out of running lab `id`, leaving the rest of the lab up.
async fn detach_box(dirs: &AppDirs, id: &str, runtime: Runtime, log: Log) -> Result<()> {
    let dir = lab_dir(dirs, id)?;
    let _lock = crate::runtime::lock_lab(dirs, id).await?;
    let run = crate::runtime::attack_box::reachable(dirs, &dir, id, runtime)?;
    let mut line = move |l: String| log(l);
    Box::pin(run.remove(dirs, &mut line)).await
}
