//! A run's operations through its home's runner service (`isothrum-client`): the one process that
//! runs a home's operations, queued per run, so they outlive the window (and the app) that asked
//! for them. The launcher hosts the service itself (`run_service::host`), started on first use by
//! [`Client::connect_exact`] as this executable, of this release. An operation is a command on the
//! wire (the run's folder, format and name, its values and secrets), its events read back as the
//! deploy log's lines. When no service can be reached the operation runs in this process instead,
//! said in the log: the runner's own per-run lock keeps that safe beside a service.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use isothrum_client::proto::{Command, OpOptions, Target};
use isothrum_client::{Client, ClientError};
use isothrum_runner::{ErrorCode, ErrorDetail, EventKind, Format, RunName, RunOptions, Running, Status};
use tokio::sync::Mutex as AsyncMutex;

use super::{errors, progress, when_unlocked};
use crate::error::Error;

/// The connected services, by home: one connection per home in this process.
static CLIENTS: LazyLock<AsyncMutex<HashMap<PathBuf, Arc<Client>>>> = LazyLock::new(|| AsyncMutex::new(HashMap::new()));

/// The runner service of `home`, connected (started first when none runs: this executable, as
/// `serve --home <home> --detach`); none when it can't be, said in the app's log, for the caller
/// to run the operation in this process.
pub(super) async fn client(home: &Path) -> Option<Arc<Client>> {
    let mut clients = CLIENTS.lock().await;
    if let Some(client) = clients.get(home) {
        return Some(client.clone());
    }
    let exe = std::env::current_exe().ok();
    match Client::connect_exact(home, exe.as_deref()).await {
        Ok(client) => {
            let client = Arc::new(client);
            clients.insert(home.to_path_buf(), client.clone());
            Some(client)
        }
        Err(e) => {
            log::warn!("the runner service of {} can't be reached ({e}): the operation runs in this process", home.display());
            None
        }
    }
}

/// Forgets `home`'s connection, broken: the next operation connects again.
async fn forget(home: &Path) {
    CLIENTS.lock().await.remove(home);
}

/// An operation on a run, as either side runs it.
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
    /// Run its set-up again on a machine (or all), one provisioner (or all).
    Provision(Option<&'a str>, Option<&'a str>),
    /// Run a command in a machine (none: its own command, a one-off of a Compose service).
    Exec(&'a str, &'a [String]),
}

/// Why an operation failed: the runner's error (in this process), the service's (its code and
/// words), or the launcher's own (the run couldn't be opened).
#[derive(Debug)]
pub(super) enum Failure {
    /// The runner, in this process.
    Runner(isothrum_runner::Error),
    /// The runner service.
    Service {
        /// Its code.
        code: ErrorCode,
        /// Its words.
        message: String,
        /// The step that failed, when one did.
        step: Option<String>,
        /// Its typed fields, when it has any.
        detail: Option<ErrorDetail>,
    },
    /// The launcher's.
    Launcher(Error),
}

impl Failure {
    /// Its code, when the runner gave one.
    pub(super) fn code(&self) -> Option<ErrorCode> {
        match self {
            Failure::Runner(e) => Some(e.code()),
            Failure::Service { code, .. } => Some(*code),
            Failure::Launcher(_) => None,
        }
    }

    /// The launcher's error for it.
    pub(super) fn into_error(self) -> Error {
        match self {
            Failure::Runner(e) => errors::from_runner(e),
            Failure::Service { code, message, step, detail } => errors::from_service(code, &message, step.as_deref(), detail.as_ref()),
            Failure::Launcher(e) => e,
        }
    }

    /// A client call that failed (the service refused it, or the connection broke).
    fn of_client(e: ClientError) -> Failure {
        match e {
            ClientError::Operation { code, message, step, detail } => Failure::Service { code, message, step, detail },
            ClientError::Refused { code, message, detail } => Failure::Service { code, message, step: None, detail },
            other => Failure::Launcher(Error::Invalid(format!("the runner service: {other}"))),
        }
    }
}

/// The run `name` of the project in folder `project`, of `format`, as the wire names it.
pub(super) fn target(project: &Path, format: Format, name: &RunName) -> Target {
    Target::new(project, Some(name.clone())).with_format(format)
}

/// `opts` on the wire (secrets travel only over the owner-only endpoint).
fn wire(opts: &RunOptions) -> OpOptions {
    let mut wired = OpOptions::default();
    wired.provider = opts.provider;
    wired.env.clone_from(&opts.env);
    wired.secrets = opts.secrets.iter().map(|(k, v)| (k.to_owned(), v.expose().to_owned())).collect();
    wired.host_label.clone_from(&opts.host_label);
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
        Act::Provision(machine, with) => Command::Provision { target, options, machine: machine.map(str::to_owned), with: with.map(str::to_owned) },
        Act::Exec(machine, argv) => Command::Exec { target, options, machine: machine.to_owned(), argv: argv.to_vec() },
    }
}

/// Runs `act` on `target` with `opts` through `home`'s service `client`, its events as `log`'s
/// lines: whether it reached its goal (false: a check failed).
pub(super) async fn operate(
    home: &Path,
    client: &Client,
    act: Act<'_>,
    target: Target,
    opts: &RunOptions,
    log: &mut impl FnMut(String),
) -> Result<bool, Failure> {
    let mut followed = match client.follow(command(act, target, opts)).await {
        Ok(followed) => followed,
        Err(e) => {
            if matches!(e, ClientError::Io(_) | ClientError::Protocol(_)) {
                forget(home).await;
            }
            return Err(Failure::of_client(e));
        }
    };
    let mut outcome = None;
    while let Some(event) = followed.next().await {
        let event = event.map_err(Failure::of_client)?;
        if let Some(line) = progress::line(&event.kind) {
            log(line);
        }
        match event.kind {
            EventKind::Result { ok, .. } => outcome = Some(Ok(ok)),
            EventKind::Error { code, message, step, detail, .. } => outcome = Some(Err(Failure::Service { code, message, step, detail })),
            _ => {}
        }
    }
    let cancelled = || Failure::Service { code: ErrorCode::Cancelled, message: "the operation was cancelled before it began".into(), step: None, detail: None };
    outcome.unwrap_or_else(|| Err(cancelled()))
}

/// Runs `act` on `lab` in this process with `opts`, its events as `log`'s lines, waiting out
/// another operation's hold on the run: whether it reached its goal (false: a check failed).
pub(super) async fn in_process(lab: &Running<'_>, act: Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<bool, Failure> {
    let done = |r: std::result::Result<(), isothrum_runner::Error>| r.map(|()| true);
    let ran = match act {
        Act::Up => when_unlocked(log, move |sink| async move { lab.up(opts, &sink).await.map(drop) }).await.map(|()| true),
        Act::Down => done(when_unlocked(log, move |sink| async move { lab.down(opts, &sink).await }).await),
        Act::Park(mode) => done(when_unlocked(log, move |sink| async move { lab.park(mode, opts, &sink).await }).await),
        Act::Resume => done(when_unlocked(log, move |sink| async move { lab.resume(opts, &sink).await }).await),
        Act::Update => done(when_unlocked(log, move |sink| async move { lab.update(opts, &sink).await }).await),
        Act::Retire(machine) => done(when_unlocked(log, move |sink| async move { lab.retire(machine, opts, &sink).await }).await),
        Act::Provision(machine, with) => done(when_unlocked(log, move |sink| async move { lab.provision(machine, with, opts, &sink).await }).await),
        Act::Exec(machine, argv) => {
            let report = when_unlocked(log, move |sink| async move { lab.exec(machine, argv, opts, &sink).await }).await;
            report.map(|report| {
                report.stdout.lines().chain(report.stderr.lines()).for_each(|l| log(l.to_owned()));
                report.exit == 0
            })
        }
    };
    ran.map_err(Failure::Runner)
}

/// The status of run `name` (`target`) as `home`'s service `client` reads it.
pub(super) async fn status(home: &Path, client: &Client, name: &RunName, target: Target) -> Result<Status, Failure> {
    match client.status(name, Some(target)).await {
        Ok(status) => Ok(status),
        Err(e) => {
            if matches!(e, ClientError::Io(_) | ClientError::Protocol(_)) {
                forget(home).await;
            }
            Err(Failure::of_client(e))
        }
    }
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

/// The runs of `home`'s service with an operation queued or running; none when no service runs.
///
/// # Errors
///
/// The service's runs can't be listed.
pub(super) async fn busy_runs(home: &Path) -> Result<Vec<RunName>, String> {
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
pub(super) async fn journal(home: &Path, name: &RunName) -> Result<Vec<isothrum_client::v1::OperationRow>, String> {
    let Some(client) = running_service(home).await else { return Ok(Vec::new()) };
    client.operations(name, 20).await.map_err(|e| e.to_string())
}

/// Follows the operations of run `name` under way in `home`'s service to their end, their lines
/// (those already told included) as `log`'s; nothing when none is under way.
///
/// # Errors
///
/// The service's stream can't be read.
pub(super) async fn follow_under_way(home: &Path, name: &RunName, log: &mut impl FnMut(String)) -> Result<(), String> {
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
