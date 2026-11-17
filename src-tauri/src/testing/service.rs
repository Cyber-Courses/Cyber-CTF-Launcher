//! A home's runner service in this process, for tests. Everything the app asks of the runner goes
//! to its home's service (`runtime::isothrum::remote`); the app starts it as its own executable,
//! which a test binary isn't. A test that scripts the runner's tools serves the home itself
//! ([`serve_home`], [`serve`]: a runner of its own config), on the test's own runtime, so a test on
//! Tokio's paused clock runs its service on that clock too; any other home is served on demand,
//! on this host's tools as the app's service is ([`client`]), and stops once idle.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
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
    let serving = start(crate::run_service::host::runner_config(home, &processes), Some(IDLE), None, Here::OwnThread);
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

/// A service started for a test: on the test's runtime, or on a thread of its own.
enum Serving {
    /// A task of the test's runtime.
    Task(tokio::task::JoinHandle<()>),
    /// A thread of its own (no runtime to run it on).
    Thread(std::thread::JoinHandle<()>),
}

impl Serving {
    /// Whether it ended.
    fn is_finished(&self) -> bool {
        match self {
            Serving::Task(task) => task.is_finished(),
            Serving::Thread(thread) => thread.is_finished(),
        }
    }
}

/// Where a test's service runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Here {
    /// On the test's own runtime (a thread of its own when it has none): a scripted runner's, on
    /// the test's clock (Tokio's paused one included).
    TestRuntime,
    /// On a thread of its own: one on this host's tools, which the test's runtime may block while
    /// it waits on them (a lock, a tool).
    OwnThread,
}

/// Serves `config.home` `here` until `stopped` fires or its sender drops, or once `idle` passed
/// with no client; a home another service holds for good is left to it.
fn start(config: RunnerConfig, idle: Option<Duration>, stopped: Option<tokio::sync::oneshot::Receiver<()>>, here: Here) -> Serving {
    let serving = run(config, idle, stopped);
    match tokio::runtime::Handle::try_current() {
        Ok(runtime) if here == Here::TestRuntime => Serving::Task(runtime.spawn(serving)),
        _ => Serving::Thread(std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(serving);
        })),
    }
}

/// Serves `config.home` (see [`start`]).
async fn run(config: RunnerConfig, idle: Option<Duration>, stopped: Option<tokio::sync::oneshot::Receiver<()>>) {
    // A service of this home that is still stopping holds it a moment longer.
    let mut looks = 0;
    let service = loop {
        match Service::hold(config.home.clone()) {
            Ok(service) => break service,
            Err(_) if stopped.is_some() && looks < LOOKS => {
                looks += 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(_) => return,
        }
    };
    let Ok(runner) = Runner::new(config) else { return };
    let mut stopped = stopped;
    let stop: StopSignal = Box::new(move || {
        let once = stopped.take();
        Box::pin(async move {
            match once {
                Some(rx) => drop(rx.await),
                None => std::future::pending::<()>().await,
            }
        })
    });
    let mut options = ServeOptions::new(Arc::new(RunnerEngine::new(Arc::new(runner))), stop);
    options.idle = idle;
    // The app reaps its own expired labs, with the accounts' credentials.
    options.reap_every = None;
    if let Err(e) = service.serve(options).await {
        log::warn!("a test's runner service ended: {e}");
    }
}
