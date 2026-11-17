//! The one door to the runner: its home's runner service (`isothrum-client`), the one process that
//! runs a home's operations, one per run at a time (a second is refused: `locked`, the launcher's
//! `AlreadyInProgress`), so they outlive the window (and the app) that asked for them, and that
//! alone opens the home's store. The launcher hosts the service itself
//! (`run_service::host`), started on first use by [`Client::connect_exact`] as this executable, of
//! this release. An operation is a command on the wire (the run's folder, format and name, its
//! values and secrets), its events read back as the deploy log's lines; a run's record, its
//! consoles and the runner's key are asked of it the same way. A service that can't be reached is
//! [`Code::RunnerUnreachable`]: nothing runs in this process instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use isothrum_client::proto::{Command, OpOptions, Target};
use isothrum_client::{Client, ClientError, Followed, HeldConsole, Pool};
use isothrum_runner::{ErrorReport, EventKind, Format, NativeConsole, RunName, RunOptions, RunRecord, ServiceEndpoint, ShellCommand, Status};

use super::super::deploy_log;
use super::{errors, progress};
use crate::error::{Code, Error, Result};

/// The connected services, by home: one connection per home in this process, made again once
/// when it breaks. The service started when none runs is this executable's (a test serves the home
/// in its own process instead, `testing::service`).
static POOL: LazyLock<Pool> = LazyLock::new(|| {
    #[cfg(test)]
    return Pool::with(|home| Box::pin(async move { crate::testing::service::client(&home).await }));
    #[cfg(not(test))]
    Pool::new(std::env::current_exe().ok())
});

/// The runner service of `home`, connected (started first when none runs: this executable, as
/// `serve --home <home> --detach`).
///
/// # Errors
///
/// [`Code::RunnerUnreachable`] when none answers and none could be started.
pub(crate) async fn client(home: &Path) -> Result<Arc<Client>> {
    // A test's home goes with its temporary folder: its connection would hold its socket, and
    // the test's ended runtime, open for the rest of the run.
    #[cfg(test)]
    POOL.retain(crate::testing::service::kept).await;
    POOL.client(home).await.map_err(|e| unreachable(home, &e))
}

/// The error of `home`'s service that can't be reached (`e`): the app hosts it, so restarting
/// the app starts it again.
fn unreachable(home: &Path, e: &ClientError) -> Error {
    log::warn!("the runner service of {} can't be reached: {e}", home.display());
    Error::coded(Code::RunnerUnreachable { detail: e.to_string() })
}

/// The service operations this process queued (its own, told by its run service already), by id.
static OURS: LazyLock<std::sync::Mutex<std::collections::HashSet<i64>>> = LazyLock::new(Default::default);

/// Whether the service's operation `id` is one this process queued.
fn ours(id: isothrum_client::v1::OperationId) -> bool {
    OURS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&id.0)
}

/// Stops `home`'s service and forgets its connection, for a test that changes the home under it
/// (a store damaged): the next call starts a service again. Returns once the service has ended,
/// not once it stopped answering: it still writes its store, and the mirror the store is
/// repaired from, as it lets the home go, which would undo what the test changes.
#[cfg(test)]
pub(crate) async fn stop_service(home: &Path) {
    POOL.forget(home).await;
    if let Ok(running) = Client::connect_running(home).await {
        drop(running.shutdown().await);
    }
    crate::testing::service::until_ended(home).await;
}

/// `ask` of `home`'s service. A connection found dead (a service killed or restarted since it
/// was made: a broken pipe) is made again, the service started again when none runs, and `ask`
/// tried once more on it ([`Pool::call`]): only a second failure is the caller's.
///
/// # Errors
///
/// No service can be reached; the service refused or failed; the connection broke twice.
async fn call<T, F>(home: &Path, ask: impl Fn(Arc<Client>) -> F) -> std::result::Result<T, Failure>
where
    F: Future<Output = std::result::Result<T, ClientError>>,
{
    // Connected first: no service to reach is the launcher's own error, as is a new connection
    // that couldn't be made once the first broke.
    client(home).await.map_err(Failure::Launcher)?;
    POOL.call(home, ask).await.map_err(|e| failure(home, e))
}

/// Queues the operation `command` with `home`'s service and follows it, on a connection made
/// again once when found dead, as [`call`] does; one sent but unanswered may have been queued, so
/// it is followed again rather than sent again ([`Pool::follow`]).
///
/// # Errors
///
/// As [`call`].
async fn follow(home: &Path, command: Command) -> std::result::Result<Followed, Failure> {
    client(home).await.map_err(Failure::Launcher)?;
    POOL.follow(home, command).await.map_err(|e| failure(home, e))
}

/// A client's failure as the launcher reports it: no service to reach is the launcher's own.
fn failure(home: &Path, e: ClientError) -> Failure {
    match e {
        ClientError::Unavailable(_) => Failure::Launcher(unreachable(home, &e)),
        e => Failure::of_client(e),
    }
}

/// An operation on a run.
#[derive(Debug, Clone, Copy)]
pub(super) enum Act<'a> {
    /// Start it (a parked run is resumed).
    Up,
    /// Remove everything it created.
    Down,
    /// Stop it, keeping what it created.
    Park(isothrum_runner::Park),
    /// Start a parked run again.
    Resume,
    /// Bring it up to date with its project.
    Update,
    /// Take a machine out of it.
    Retire(&'a str),
    /// Start a machine the run leaves down at its start (Isoloom's controller, halted once it set
    /// the lab up).
    Attach(&'a str),
    /// Run its set-up again on a machine (or all), one provisioner (or all).
    Provision(Option<&'a str>, Option<&'a str>),
    /// Run a command in a machine (none: its own command, a one-off of a Compose service).
    Exec(&'a str, &'a [String]),
}

/// Why an operation failed: the service's report (its code and words), or the launcher's own
/// (no service, a broken connection).
#[derive(Debug)]
pub(super) enum Failure {
    /// The runner service: the runner's report (boxed: it is large, and rare).
    Service(Box<ErrorReport>),
    /// The launcher's.
    Launcher(Error),
}

impl Failure {
    /// The launcher's error for it.
    pub(super) fn into_error(self) -> Error {
        match self {
            Failure::Service(report) => errors::coded(&report),
            Failure::Launcher(e) => e,
        }
    }

    /// Whether the run may have changed: the runner's report says; a failure of the launcher's
    /// own (the run not opened, the service unreachable) is taken as one that may have.
    pub(super) fn run_changed(&self) -> bool {
        match self {
            Failure::Service(report) => report.changed,
            Failure::Launcher(_) => true,
        }
    }

    /// A client call that failed (the service refused it, or the connection broke).
    fn of_client(e: ClientError) -> Failure {
        match e {
            ClientError::Operation(report) | ClientError::Refused(report) => Failure::Service(report),
            other => Failure::Launcher(Error::Invalid(format!("the runner service: {other}"))),
        }
    }
}

/// `opts` on the wire (secrets travel only over the owner-only endpoint).
fn wire(opts: &RunOptions) -> OpOptions {
    let mut wired = OpOptions { provider: opts.provider, ..OpOptions::default() };
    wired.env.clone_from(&opts.env);
    wired.secrets = opts.secrets.iter().map(|(k, v)| (k.to_owned(), v.expose().to_owned())).collect();
    wired.host_label.clone_from(&opts.host_label);
    wired.reveal = opts.reveal;
    wired.docker_context.clone_from(&opts.docker_context);
    wired
}

/// `act` on `target` as a command of the wire, with `opts`.
fn command(act: Act<'_>, target: Target, opts: &RunOptions) -> Command {
    let options = wire(opts);
    match act {
        Act::Up => Command::Up { target, options, lifetime_s: opts.lifetime.map(|l| l.as_secs()) },
        Act::Down => Command::Down { target, options },
        Act::Park(mode) => Command::Park { target, options, mode },
        Act::Resume => Command::Resume { target, options },
        Act::Update => Command::Update { target, options },
        Act::Retire(machine) => Command::Retire { target, options, machine: machine.to_owned() },
        Act::Attach(machine) => Command::Attach { target, options, machine: machine.to_owned() },
        Act::Provision(machine, with) => Command::Provision { target, options, machine: machine.map(str::to_owned), with: with.map(str::to_owned) },
        Act::Exec(machine, argv) => Command::Exec { target, options, machine: machine.to_owned(), argv: argv.to_vec() },
    }
}

/// A run as its home's service knows it: the home, and the run's project (its folder and format)
/// under its name.
#[derive(Debug, Clone)]
pub(super) struct Served {
    /// The runner's home.
    pub(super) home: PathBuf,
    /// The run's name.
    pub(super) name: RunName,
    /// Its project's folder.
    pub(super) project: PathBuf,
    /// Its project's format.
    pub(super) format: Format,
}

impl Served {
    /// The run as the wire names it.
    fn target(&self) -> Target {
        Target::new(&self.project, Some(self.name.clone())).with_format(self.format)
    }

    /// Runs `act` with `opts` on the run, its events as `log`'s lines and its steps told to the
    /// operation's ([`deploy_log::tell`]): whether it reached its goal (false: a check failed).
    pub(super) async fn operate(&self, act: Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> std::result::Result<bool, Failure> {
        // Queued on a connection that answers: one found dead first is made again, and one that
        // ended before the service answered is followed again, not queued twice.
        let mut followed = follow(&self.home, command(act, self.target(), opts)).await?;
        OURS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(followed.accepted.operation.0);
        let mut outcome = None;
        while let Some(event) = followed.next().await {
            let event = event.map_err(Failure::of_client)?;
            if let Some(line) = progress::line(&event.kind) {
                log(line);
            }
            if let Some(step) = progress::step(&event.kind) {
                deploy_log::tell(step);
            }
            match event.kind {
                EventKind::Result { ok, .. } => outcome = Some(Ok(ok)),
                EventKind::Error(report) => outcome = Some(Err(Failure::Service(Box::new(report)))),
                _ => {}
            }
        }
        outcome.unwrap_or_else(|| Err(Failure::Service(Box::new(errors::cancelled_before_it_began()))))
    }

    /// The run's status, its tools given `opts`' values and secrets.
    pub(super) async fn status(&self, opts: &RunOptions) -> std::result::Result<Status, Failure> {
        call(&self.home, |client| async move { client.status_with(&self.name, Some(self.target()), wire(opts)).await }).await
    }

    /// The run's record, none before it ever ran.
    pub(super) async fn record(&self) -> Result<Option<RunRecord>> {
        record(&self.home, &self.name).await
    }

    /// The command that opens a console into `machine`, as account `user` when given.
    pub(super) async fn console_command(&self, machine: &str, user: Option<&str>) -> std::result::Result<ShellCommand, Failure> {
        call(&self.home, |client| async move { client.console_command(&self.name, Some(self.target()), machine, user).await }).await
    }

    /// `machine`'s graphical console, its hypervisor asked with `opts`, held (with its tunnel)
    /// until dropped.
    pub(super) async fn graphical_console(&self, machine: &str, opts: &RunOptions) -> std::result::Result<HeldConsole, Failure> {
        call(&self.home, |client| async move { client.graphical_console(&self.name, Some(self.target()), wire(opts), machine).await }).await
    }

    /// What opens running VM `machine` in its hypervisor's own app, asked with `opts`.
    pub(super) async fn native_console(&self, machine: &str, opts: &RunOptions) -> std::result::Result<NativeConsole, Failure> {
        call(&self.home, |client| async move { client.native_console(&self.name, Some(self.target()), wire(opts), machine).await }).await
    }

    /// The service endpoints the run publishes, each where this computer reaches it (a forward
    /// the service keeps open), its outputs read with `opts`.
    pub(super) async fn service_endpoints(&self, opts: &RunOptions) -> std::result::Result<Vec<ServiceEndpoint>, Failure> {
        call(&self.home, |client| async move { client.service_endpoints(&self.name, Some(self.target()), wire(opts)).await }).await
    }
}

/// Every run `home`'s service records, each in the state its last operation left it in.
///
/// # Errors
///
/// No service can be reached, or it can't read its records.
pub(crate) async fn records(home: &Path) -> Result<Vec<RunRecord>> {
    Ok(rows(home).await?.into_iter().map(|r| r.record).collect())
}

/// Every run `home`'s service records, as it last saw each.
///
/// # Errors
///
/// No service can be reached, or it can't read its records.
pub(crate) async fn rows(home: &Path) -> Result<Vec<isothrum_runner::store::RunRow>> {
    call(home, |client| async move { client.list().await }).await.map_err(Failure::into_error)
}

/// What each running machine of run `name` in `home`'s service uses now, as its engine samples
/// it (none for a run whose tool reports no per-machine figure).
///
/// # Errors
///
/// No service can be reached, or the run's engine doesn't answer.
pub(crate) async fn usage(home: &Path, name: &RunName) -> Result<Vec<isothrum_runner::MachineUsage>> {
    call(home, |client| async move { client.usage(name, None, OpOptions::default()).await }).await.map_err(Failure::into_error)
}

/// Run `name`'s record in `home`'s service, none for a run never recorded.
///
/// # Errors
///
/// No service can be reached, or it can't read the record.
pub(crate) async fn record(home: &Path, name: &RunName) -> Result<Option<RunRecord>> {
    Ok(call(home, |client| async move { client.record(name).await }).await.map_err(Failure::into_error)?.map(|r| r.record))
}

/// Forgets run `name` in `home`'s service.
///
/// # Errors
///
/// No service can be reached; an operation of the run is under way; the store can't be written.
pub(crate) async fn forget_run(home: &Path, name: &RunName) -> Result<()> {
    call(home, |client| async move { client.forget(name).await }).await.map_err(Failure::into_error)
}

/// The public half of the runner's SSH key (the launcher's own), made when there is none.
///
/// # Errors
///
/// No service can be reached; the key can't be made.
pub(crate) async fn public_key(home: &Path) -> Result<String> {
    call(home, |client| async move { client.public_key().await }).await.map_err(Failure::into_error)
}

/// What `home`'s runner downloaded and records (its runs' images and boxes, and those adopted),
/// each with its size when it can be read.
///
/// # Errors
///
/// No service can be reached, or it can't read its record.
pub(crate) async fn downloads(home: &Path) -> Result<Vec<isothrum_runner::Download>> {
    call(home, |client| async move { client.downloads().await }).await.map_err(Failure::into_error)
}

/// Removes the downloads `home`'s runner records that `which` names: how each went.
///
/// # Errors
///
/// No service can be reached, or it can't read or write its record.
pub(crate) async fn remove_downloads(home: &Path, which: isothrum_runner::DownloadsToRemove) -> Result<Vec<isothrum_runner::DownloadRemoval>> {
    call(home, |client| {
        let which = which.clone();
        async move { client.remove_downloads(which).await }
    })
    .await
    .map_err(Failure::into_error)
}

/// Adopts `adopted` into `home`'s runner's record of downloads: those on this machine now, as
/// they were fetched.
///
/// # Errors
///
/// No service can be reached; the engine or Vagrant couldn't say what they hold; the record can't
/// be written.
pub(crate) async fn adopt_downloads(home: &Path, adopted: Vec<isothrum_runner::DownloadToAdopt>) -> Result<Vec<isothrum_runner::Download>> {
    call(home, |client| {
        let adopted = adopted.clone();
        async move { client.adopt_downloads(adopted).await }
    })
    .await
    .map_err(Failure::into_error)
}

/// The IPv4 address blocks the networks of the engine hold, as `home`'s runner reads them, but
/// those of run `except`'s own.
///
/// # Errors
///
/// No service can be reached, Docker is missing, or the engine can't say.
pub(crate) async fn address_blocks_in_use(home: &Path, except: &RunName) -> Result<Vec<String>> {
    call(home, |client| {
        let except = except.clone();
        async move { client.address_blocks_in_use(Some(except), None).await }
    })
    .await
    .map_err(Failure::into_error)
}

/// The recorded runs of `home`'s service past their lifetime that may still hold something.
///
/// # Errors
///
/// No service can be reached, or it can't read its records.
pub(crate) async fn expired(home: &Path) -> Result<Vec<RunRecord>> {
    Ok(call(home, |client| async move { client.expired().await }).await.map_err(Failure::into_error)?.into_iter().map(|r| r.record).collect())
}

/// `home`'s runner service when one runs (none is started to answer a question about it).
async fn running_service(home: &Path) -> Option<Client> {
    Client::connect_running(home).await.ok()
}

/// The operations of run `name` queued or running in `client`'s service.
async fn under_way(client: &Client, name: &RunName) -> Vec<isothrum_client::v1::OperationId> {
    match client.operations(name, 16).await {
        Ok(rows) => rows.into_iter().filter(|o| o.ended.is_none()).map(|o| o.id).collect(),
        Err(e) => {
            log::warn!("the runner service's operations of {name} can't be read: {e}");
            Vec::new()
        }
    }
}

/// Cancels every operation of run `name` queued or running in `home`'s service: a stop ends what
/// runs on the run first (a launch's `up` cut short, its tools stopped).
pub(super) async fn cancel_under_way(home: &Path, name: &RunName) {
    let Some(client) = running_service(home).await else { return };
    for operation in under_way(&client, name).await {
        if let Err(e) = client.cancel(operation).await {
            log::warn!("{name}: the runner service's operation {operation:?} couldn't be cancelled: {e}");
        }
    }
}

/// Whether run `name` has an operation queued or running in `home`'s service; false when none
/// runs.
pub(super) async fn run_busy(home: &Path, name: &RunName) -> bool {
    let Some(client) = running_service(home).await else { return false };
    !under_way(&client, name).await.is_empty()
}

/// The runs of `home`'s service with an operation queued or running; none when no service runs.
///
/// # Errors
///
/// The service's runs can't be listed.
pub(super) async fn busy_runs(home: &Path) -> std::result::Result<Vec<RunName>, String> {
    let Some(client) = running_service(home).await else { return Ok(Vec::new()) };
    let runs = client.list().await.map_err(|e| e.to_string())?;
    let mut busy = Vec::new();
    for run in runs {
        if !under_way(&client, &run.name).await.is_empty() {
            busy.push(run.name);
        }
    }
    Ok(busy)
}

/// Run `name`'s operations as `home`'s service journals them, newest first (none when no service
/// runs).
///
/// # Errors
///
/// The journal can't be read.
pub(super) async fn journal(home: &Path, name: &RunName) -> std::result::Result<Vec<isothrum_client::v1::OperationRow>, String> {
    let Some(client) = running_service(home).await else { return Ok(Vec::new()) };
    client.operations(name, 20).await.map_err(|e| e.to_string())
}

/// Follows the operations of run `name` under way in `home`'s service to their end, their lines
/// (those already told included) as `log`'s; nothing when none is under way.
///
/// # Errors
///
/// The service's stream can't be read.
pub(super) async fn follow_under_way(home: &Path, name: &RunName, log: &mut impl FnMut(String)) -> std::result::Result<(), String> {
    let Some(client) = running_service(home).await else { return Ok(()) };
    let mut left = under_way(&client, name).await;
    if left.is_empty() {
        return Ok(());
    }
    let mut events = client.subscribe(Some(name.clone()), None).await.map_err(|e| e.to_string())?;
    while let Some(item) = events.next().await {
        let (_, event) = item.map_err(|e| e.to_string())?;
        let Some(operation) = event.operation.filter(|o| left.contains(o)) else { continue };
        if let Some(line) = progress::line(&event.kind) {
            log(line);
        }
        if isothrum_client::v1::phase_of(&event) == Some(isothrum_runner::OperationPhase::Ended) {
            left.retain(|o| *o != operation);
            if left.is_empty() {
                break;
            }
        }
    }
    Ok(())
}

/// What the service does that this process didn't ask for, as the app tells it.
#[derive(Debug)]
pub(crate) enum Others {
    /// An operation another process queued (the command line, an earlier run of the app) began.
    Began {
        /// The service's id for it.
        operation: i64,
        /// Its run.
        run: String,
        /// What it does.
        op: isothrum_runner::Op,
    },
    /// One of its lines.
    Line {
        /// The service's id for it.
        operation: i64,
        /// The line.
        line: String,
    },
    /// One of its steps.
    Step {
        /// The service's id for it.
        operation: i64,
        /// The step.
        step: deploy_log::DeployStep,
    },
    /// It ended (failed with the error).
    Ended {
        /// The service's id for it.
        operation: i64,
        /// Why it failed.
        error: Option<Error>,
    },
    /// Run `run` left its standing state (ready or parked): taken down, a machine retired, failed.
    Left {
        /// The run.
        run: String,
    },
}

/// Follows `home`'s service (`client`) for as long as its stream lasts, telling `on` what
/// another process asked of it and when a run leaves its standing state. Only what happens from
/// now on, and the operations under way now, are told: the service's history isn't replayed.
///
/// # Errors
///
/// The service's stream can't be read, or ended.
pub(crate) async fn follow_others(client: &Client, on: &mut impl FnMut(Others)) -> std::result::Result<(), String> {
    let mut under = std::collections::HashSet::new();
    for run in client.list().await.map_err(|e| e.to_string())? {
        under.extend(under_way(client, &run.name).await.into_iter().map(|o| o.0));
    }
    let mut others = OthersTold::since_now(under);
    let mut events = client.subscribe(None, None).await.map_err(|e| e.to_string())?;
    while let Some(item) = events.next().await {
        let (_, event) = item.map_err(|e| e.to_string())?;
        let mine = event.operation.is_some_and(ours);
        others.take(&event, mine, on);
    }
    Err("the runner service's stream ended".into())
}

/// What [`follow_others`] tells of the service's events, one after the other: from when it began
/// (`since`), the operations then under way (`under`) included, and those it told begun and not
/// yet ended, each with the error it ended on so far (`told`).
struct OthersTold {
    /// When the following began: what happened before isn't told.
    since: String,
    /// The operations under way when it began, told although they began before.
    under: std::collections::HashSet<i64>,
    /// The operations told begun and not yet ended, with their error so far.
    told: HashMap<i64, Option<Error>>,
}

impl OthersTold {
    /// Following from now, `under` the operations under way.
    fn since_now(under: std::collections::HashSet<i64>) -> OthersTold {
        OthersTold { since: isothrum_runner::rfc3339(std::time::SystemTime::now()), under, told: HashMap::new() }
    }

    /// Tells `on` what `event` says that this process didn't ask for; `mine` when its operation
    /// is one this process queued ([`ours`]). The service's stream can carry an operation's first
    /// events before its queuer has its id: such an operation of ours, told begun as another's,
    /// is told ended as soon as it shows to be ours, as its queuer tells it from there on. Left in
    /// flight, its lab read busy and starting for as long as the app ran.
    fn take(&mut self, event: &isothrum_runner::Event, mine: bool, on: &mut impl FnMut(Others)) {
        use isothrum_runner::{OperationPhase, RunState};
        let live = event.time >= self.since;
        let standing = |s: &RunState| matches!(s, RunState::Ready | RunState::Parked);
        if let (EventKind::State { from, to }, Some(run), true) = (&event.kind, &event.run, live)
            && standing(from)
            && !standing(to)
        {
            on(Others::Left { run: run.clone() });
        }
        let Some(id) = event.operation.map(|o| o.0) else { return };
        if mine {
            if self.told.remove(&id).is_some() {
                on(Others::Ended { operation: id, error: None });
            }
            return;
        }
        let ended = isothrum_client::v1::phase_of(event) == Some(OperationPhase::Ended);
        if let std::collections::hash_map::Entry::Vacant(first) = self.told.entry(id) {
            if !(live || self.under.contains(&id)) || ended {
                return;
            }
            let (Some(run), Some(op)) = (event.run.clone(), event.op) else { return };
            first.insert(None);
            on(Others::Began { operation: id, run, op });
        }
        if let Some(line) = progress::line(&event.kind) {
            on(Others::Line { operation: id, line });
        }
        if let Some(step) = progress::step(&event.kind) {
            on(Others::Step { operation: id, step });
        }
        if let EventKind::Error(report) = &event.kind {
            self.told.insert(id, Some(errors::coded(report)));
        }
        if ended {
            let error = self.told.remove(&id).flatten();
            on(Others::Ended { operation: id, error });
        }
    }
}

#[cfg(test)]
mod tests;
