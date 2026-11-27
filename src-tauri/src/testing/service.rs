//! A home's runner service in this process, for tests. Everything the app asks of the runner goes
//! to its home's service (`runtime::isothrum::remote`); the app starts it as its own executable,
//! which a test binary isn't. A test that scripts the runner's tools serves the home itself
//! ([`serve_home`], [`serve`]: a runner of its own config), on the test's own runtime, so a test on
//! Tokio's paused clock runs its service on that clock too; any other home is served on demand,
//! on this host's tools as the app's service is ([`client`]), and stops once idle or once its
//! home's folder is gone (the test's temporary folder dropped). Only Unix removes that folder while
//! the service holds its store open: Windows refuses to remove an open file, so there the folder
//! is removed once the service has ended (`testing::retry_removals`), which it does with its
//! test's runtime (whose end waits for it) or once idle.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use isothrum_client::{Client, ClientError};
use isothrum_runner::{Runner, RunnerConfig, SystemProcesses};
use isothrum_service::{RunnerEngine, ServeOptions, Service, StopSignal};

/// How long a service started on demand waits with no client before it stops.
const IDLE: Duration = Duration::from_secs(10);

/// How many times a test waits for a service it started to answer, a moment each.
const LOOKS: usize = 6000;

/// The homes a test serves with a runner of its own: [`client`] waits for theirs, never starting
/// one on this host's tools.
static SERVED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// A service a test serves: stopped when dropped.
pub struct Served {
    _stop: tokio::sync::oneshot::Sender<()>,
}

/// Serves `config.home` in this process, on a runner of `config` (its scripted tools), until the
/// [`Served`] drops.
pub fn serve(config: RunnerConfig) -> Served {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    register(&config.home);
    drop(start(config, None, Some(stopped), Here::TestRuntime));
    Served { _stop: stop }
}

/// Serves `config.home` as [`serve`] does, for as long as the test's runtime runs, unless this
/// process serves it already: a test's scripted runs open the same home again and again.
pub fn serve_home(config: RunnerConfig) {
    if register(&config.home) {
        drop(start(config, None, None, Here::TestRuntime));
    }
}

/// Serves `home` as [`serve_home`] does, on a runner whose every tool answers with nothing (an
/// engine with no network, Vagrant with no box): for a test that only needs the runner never to
/// reach this host's tools.
pub fn serve_silent(home: &Path) {
    let mut config = RunnerConfig::new(home);
    config.processes = Arc::new(isothrum_runner::fake::FakeProcesses::model(|_| isothrum_runner::fake::Reply::ok("")));
    serve_home(config);
}

/// Notes `home` as served by a test; false when it was already.
fn register(home: &Path) -> bool {
    std::fs::create_dir_all(home).unwrap();
    SERVED.lock().unwrap_or_else(PoisonError::into_inner).insert(home.to_path_buf())
}

/// The service of `home`, connected: the one that runs (or that a test starts for it), else one
/// started on this host's tools.
///
/// # Errors
///
/// None answers in time.
pub async fn client(home: &Path) -> Result<Client, ClientError> {
    if let Ok(client) = Client::connect_running(home).await {
        return Ok(client);
    }
    if SERVED.lock().unwrap_or_else(PoisonError::into_inner).contains(home) {
        return answering(home, None).await;
    }
    std::fs::create_dir_all(home).map_err(|e| ClientError::Unavailable(format!("{}: {e}", home.display())))?;
    let processes = SystemProcesses::default();
    let serving = start(crate::run_service::host::runner_config(home, &processes), Some(IDLE), None, Here::Services);
    answering(home, Some(&serving)).await
}

/// A client of `home`'s service once it answers; an error when `serving` ended first (it couldn't
/// serve: its store can't be opened), or none answers in time.
async fn answering(home: &Path, serving: Option<&Serving>) -> Result<Client, ClientError> {
    let mut looks = 0;
    loop {
        let ended = serving.is_some_and(Serving::is_finished);
        match Client::connect_running(home).await {
            Ok(client) => return Ok(client),
            Err(e) if ended || looks >= LOOKS => return Err(e),
            Err(_) => {
                looks += 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

/// A service started for a test: a task of the test's runtime or of the services' one.
type Serving = tokio::task::JoinHandle<()>;

/// Where a test's service runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Here {
    /// On the test's own runtime (the services' one when it has none): a scripted runner's, on
    /// the test's clock (Tokio's paused one included).
    TestRuntime,
    /// On the services' runtime: one on this host's tools, which the test's runtime may block
    /// while it waits on them (a lock, a tool).
    Services,
}

/// What ends a service a test started: its stop firing (or its sender dropping), and for one on
/// the services' runtime its test's runtime ending or its home's folder going too.
type Until = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

/// How often a service on the services' runtime looks whether its home's folder is still there.
const GONE_EVERY: Duration = Duration::from_millis(50);

/// The runtime of the services that don't run on their test's: one for the process. A runtime
/// (and a thread) of each one's own held two kqueues as long as it served, and the tests running
/// at once then came near macOS's 256 open files.
fn services_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(4).thread_name("test-services").enable_all().build().unwrap())
}

/// Serves `config.home` `here` until `stopped` fires or its sender drops, or once `idle` passed
/// with no client; a home another service holds for good is left to it. One on the test's
/// runtime ends with it; one on the services' runtime ends with the runtime it was started from
/// (the test's), or once its home's folder is gone (the test's temporary folder dropped, for one
/// started from the app's runtime, which runs for the whole run): it would hold its store and its
/// socket open for the rest of the run otherwise.
fn start(config: RunnerConfig, idle: Option<Duration>, stopped: Option<tokio::sync::oneshot::Receiver<()>>, here: Here) -> Serving {
    let waits = stopped.is_some();
    let stopped = async move {
        match stopped {
            Some(rx) => drop(rx.await),
            None => std::future::pending::<()>().await,
        }
    };
    let runtime = tokio::runtime::Handle::try_current();
    match runtime {
        Ok(runtime) if here == Here::TestRuntime => runtime.spawn(run(config, idle, waits, Box::pin(stopped))),
        _ => {
            let (ended, gone) = (ended(runtime.ok(), config.home.clone()), gone(config.home.clone()));
            let until: Until = Box::pin(async move {
                tokio::select! {
                    () = stopped => {}
                    () = ended => {}
                    () = gone => {}
                }
            });
            services_runtime().spawn(run(config, idle, waits, until))
        }
    }
}

/// Once `runtime` (the test's) has ended: a task of it holds what this waits on, dropped with
/// it. Never, with no runtime. The runtime's end waits for `home`'s service to have ended too,
/// and its folders to have gone (see [`Ending`]).
fn ended(runtime: Option<tokio::runtime::Handle>, home: PathBuf) -> impl Future<Output = ()> + Send {
    let ended = runtime.map(|runtime| {
        let (held, ended) = tokio::sync::oneshot::channel::<()>();
        let ending = Ending { held: Some(held), home };
        drop(runtime.spawn(async move {
            let _ending = ending;
            std::future::pending::<()>().await;
        }));
        ended
    });
    async move {
        match ended {
            Some(ended) => drop(ended.await),
            None => std::future::pending::<()>().await,
        }
    }
}

/// Held by a task of a test's runtime for a service it started on the services' runtime: dropped
/// as that runtime ends, it tells the service to stop, then waits (within [`ENDS_WITHIN`]) for it
/// to have ended, and removes the test's folders Windows refused while the service held them
/// (`testing::retry_removals`). A test's process exits right after its runtime ends: the folders
/// left to the service's own end would stay.
struct Ending {
    held: Option<tokio::sync::oneshot::Sender<()>>,
    home: PathBuf,
}

impl Drop for Ending {
    fn drop(&mut self) {
        drop(self.held.take());
        // Unix removed every folder at once: nothing to wait for.
        if !crate::testing::removals_pending() {
            return;
        }
        let started = std::time::Instant::now();
        while RUNNING.lock().unwrap_or_else(PoisonError::into_inner).contains_key(&self.home) && started.elapsed() < ENDS_WITHIN {
            std::thread::sleep(Duration::from_millis(10));
        }
        crate::testing::retry_removals();
    }
}

/// Once `home`'s folder is gone, or let go by its test where Windows can't remove it while this
/// service holds its store (`testing::let_go`): a service started from the app's runtime, which
/// never ends, would otherwise hold it until it idled, after the test's process had exited.
async fn gone(home: PathBuf) {
    while home.exists() && !crate::testing::let_go(&home) {
        tokio::time::sleep(GONE_EVERY).await;
    }
}

/// Whether `home`'s folder is still there: a test's connection to a home that's gone is
/// forgotten (`runtime::isothrum::remote`), its service ended.
pub fn kept(home: &Path) -> bool {
    home.exists()
}

/// The services of this process still running, by home: each one's [`Running`] guard holds the
/// sender, so its receiver hears it once the service has ended.
static RUNNING: Mutex<BTreeMap<PathBuf, tokio::sync::watch::Receiver<()>>> = Mutex::new(BTreeMap::new());

/// A service of `home` in this process, running until dropped.
struct Running {
    home: PathBuf,
    ended: tokio::sync::watch::Sender<()>,
}

impl Running {
    /// Notes `home`'s service as running.
    fn of(home: &Path) -> Running {
        let (ended, running) = tokio::sync::watch::channel(());
        RUNNING.lock().unwrap_or_else(PoisonError::into_inner).insert(home.to_path_buf(), running);
        Running { home: home.to_path_buf(), ended }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        if running.get(&self.home).is_some_and(|r| r.same_channel(&self.ended.subscribe())) {
            running.remove(&self.home);
        }
    }
}

/// How long a service asked to stop may take to end: longer than Isothrum's longest stop (its
/// running operations' 40 seconds to drain, then 5 for its tools and 5 for its connections).
const ENDS_WITHIN: Duration = Duration::from_secs(60);

/// Waits until `home`'s service in this process, if one runs, has ended: it stopped answering
/// first, then still wrote its store (and its mirror) as it let the home go.
///
/// # Panics
///
/// It hasn't ended within [`ENDS_WITHIN`].
pub async fn until_ended(home: &Path) {
    let running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner).get(home).cloned();
    if let Some(mut running) = running {
        // Never sent to: it returns once the sender drops. Timed on the services' runtime, whose
        // clock is never paused: a test's paused one would skip ahead to the bound.
        let waited = services_runtime().spawn(async move { tokio::time::timeout(ENDS_WITHIN, running.changed()).await.is_ok() });
        assert!(waited.await.unwrap(), "the runner service of {} hasn't ended", home.display());
    }
}

/// Serves `config.home` (see [`start`]).
async fn run(config: RunnerConfig, idle: Option<Duration>, waits: bool, until: Until) {
    // Dropped last, once the service has let its home go (its end, or its task dropped with a
    // test's runtime): the test folders Windows wouldn't remove while it held them go then.
    let _removals = Removals;
    // A service of this home that is still stopping holds it a moment longer.
    let mut looks = 0;
    let service = loop {
        match Service::hold(config.home.clone()) {
            Ok(service) => break service,
            Err(_) if waits && looks < LOOKS => {
                looks += 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(_) => return,
        }
    };
    // Dropped once `serve` has returned: the store closed, the home let go.
    let _running = Running::of(&config.home);
    let Ok(runner) = Runner::new(config) else { return };
    let mut until = Some(until);
    let stop: StopSignal = Box::new(move || until.take().unwrap_or_else(|| Box::pin(std::future::pending())));
    let mut options = ServeOptions::new(Arc::new(RunnerEngine::new(Arc::new(runner))), stop);
    options.idle = idle;
    // The app reaps its own expired labs, with the accounts' credentials.
    options.reap_every = None;
    if let Err(e) = service.serve(options).await {
        log::warn!("a test's runner service ended: {e}");
    }
}

/// Removes, when dropped, the test folders Windows wouldn't remove before (`testing::retry_removals`).
struct Removals;

impl Drop for Removals {
    fn drop(&mut self) {
        crate::testing::retry_removals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many looks, a moment each, a service of a test that ended may take to go: well before
    /// it would idle.
    const LOOKS_BEFORE_IDLE: usize = 300;

    /// Waits for the service `answers` asks to stop answering.
    async fn stops_answering(answers: impl AsyncFn() -> bool) {
        let mut looks = 0;
        while answers().await {
            assert!(looks < LOOKS_BEFORE_IDLE, "the service of a test that ended still answers");
            looks += 1;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // A service a test left serving keeps its store and its socket open for the rest of the run:
    // a full run's then outgrew macOS's 256 open files.

    // Windows won't remove the folder while the service holds its store open (`state.db`, with
    // no sharing for deletion): the test's temporary folder outlives its drop there.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_service_started_on_demand_ends_with_its_homes_folder() {
        let dir = tempfile::tempdir().unwrap();
        // Asked on the connection made before: the service's address was in the folder.
        let client = client(&dir.path().join("isothrum")).await.unwrap();
        drop(dir);
        stops_answering(async || client.list().await.is_ok()).await;
    }

    #[test]
    fn a_service_started_on_demand_ends_with_the_tests_runtime() {
        let dir = crate::testing::temp_dir("service-runtime");
        let home = dir.join("isothrum");
        let test = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        drop(test.block_on(client(&home)).unwrap());
        drop(test);
        let after = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        after.block_on(stops_answering(async || Client::connect_running(&home).await.is_ok()));
    }
}
