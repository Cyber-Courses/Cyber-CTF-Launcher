//! A home's runner service in this process, for tests. Everything the app asks of the runner goes
//! to its home's service (`runtime::isothrum::remote`); the app starts it as its own executable,
//! which a test binary isn't. A test that scripts the runner's tools serves the home itself
//! ([`serve_home`], [`serve`]: a runner of its own config), on the test's own runtime, so a test on
//! Tokio's paused clock runs its service on that clock too; any other home is served on demand,
//! on this host's tools as the app's service is ([`client`]), and stops once idle or once its
//! home's folder is gone (the test's temporary folder dropped).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use isothrum_client::Client;
use isothrum_runner::{Runner, RunnerConfig, SystemProcesses};
use isothrum_service::{RunnerEngine, ServeOptions, Service, StopSignal};

use crate::error::{Error, Result};

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
pub async fn client(home: &Path) -> Result<Arc<Client>> {
    if let Ok(client) = Client::connect_running(home).await {
        return Ok(Arc::new(client));
    }
    if SERVED.lock().unwrap_or_else(PoisonError::into_inner).contains(home) {
        return answering(home, None).await.map(Arc::new);
    }
    std::fs::create_dir_all(home).map_err(|e| Error::Invalid(format!("{}: {e}", home.display())))?;
    let processes = SystemProcesses::default();
    let serving = start(crate::run_service::host::runner_config(home, &processes), Some(IDLE), None, Here::Services);
    answering(home, Some(&serving)).await.map(Arc::new)
}

/// A client of `home`'s service once it answers; an error when `serving` ended first (it couldn't
/// serve: its store can't be opened), or none answers in time.
async fn answering(home: &Path, serving: Option<&Serving>) -> Result<Client> {
    let mut looks = 0;
    loop {
        let ended = serving.is_some_and(Serving::is_finished);
        match Client::connect_running(home).await {
            Ok(client) => return Ok(client),
            Err(e) if ended || looks >= LOOKS => return Err(crate::runtime::isothrum::service_unreachable(home, &e)),
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
            let (ended, gone) = (ended(runtime.ok()), gone(config.home.clone()));
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
/// it. Never, with no runtime.
fn ended(runtime: Option<tokio::runtime::Handle>) -> impl Future<Output = ()> + Send {
    let ended = runtime.map(|runtime| {
        let (held, ended) = tokio::sync::oneshot::channel::<()>();
        drop(runtime.spawn(async move {
            let _held = held;
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

/// Once `home`'s folder is gone.
async fn gone(home: PathBuf) {
    while home.exists() {
        tokio::time::sleep(GONE_EVERY).await;
    }
}

/// Whether `home`'s folder is still there: a test's connection to a home that's gone is
/// forgotten (`runtime::isothrum::remote`), its service ended.
pub fn kept(home: &Path) -> bool {
    home.exists()
}

/// Serves `config.home` (see [`start`]).
async fn run(config: RunnerConfig, idle: Option<Duration>, waits: bool, until: Until) {
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
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("isothrum");
        let test = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        drop(test.block_on(client(&home)).unwrap());
        drop(test);
        let after = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        after.block_on(stops_answering(async || Client::connect_running(&home).await.is_ok()));
    }
}
