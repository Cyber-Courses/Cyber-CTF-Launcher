//! Every run operation and every run event go through one service ([`RunService`]): the commands
//! ask it to start or cancel an operation, and get back the operation's id at once; what the
//! operation does and how it ends reach every window as events ([`events`]), never through a
//! command's own channel nor a poll. A window that (re)loads replays the events it missed.
//!
//! The service behind it is the launcher's own ([`local::LocalRunService`], [`of`]): each
//! operation's launcher steps on a task of the app, what it asks of the lab's run in the home's
//! runner service (`runtime::isothrum`), which runs it to its end whatever becomes of the app. It
//! is also the one account of what this process is doing to which lab: what a quit, an update
//! install and a lab's removal wait for.

pub(crate) mod events;
pub(crate) mod host;
pub(crate) mod local;
#[cfg(test)]
pub(crate) mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::Manager;

pub(crate) use events::{Cursored, EventLog, RUN_EVENT, RunEvent};
pub(crate) use local::LocalRunService;

use crate::AppHandle;
use crate::error::{Error, Result};
use crate::labs::LaunchRequest;
use crate::runtime::{Operation, Park, Runtime};

/// An operation's id, the service's: what its events carry, what cancels it, and the one id of
/// the operation from the click that started it to every line of its log (the app's log names it
/// at its start and end; the lab page shows it on a failure, for the player to quote). The
/// runner's service hands it out when it accepts the command (its journal's id).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, ts_rs::TS)]
pub struct OperationId(pub(crate) String);

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl OperationId {
    /// A new id, unique among this app's operations.
    fn new() -> OperationId {
        OperationId(format!("op-{:016x}", rand::random::<u64>()))
    }
}

/// The learner's pick for an attach: which lab network to join (none: the seat the lab declares,
/// else its first network), with which image for a container box (none: the launcher's default;
/// a VM box is Kali whatever is picked).
#[derive(Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachPick {
    /// The lab network to join.
    pub network: Option<String>,
    /// The container image of a container box.
    pub image: Option<String>,
    /// A VM box gets Kali's desktop (the Settings choice; a container box has none). Absent:
    /// headless.
    #[serde(default)]
    pub desktop: bool,
}

/// What the launcher asks to be done to a lab.
#[derive(Debug, Clone)]
pub(crate) enum Request {
    /// Launch a lab of the catalogue.
    Launch(LaunchRequest),
    /// Import and launch a lab from folder `src` (an unpublished lab), as lab `lab_id`.
    LaunchLocal {
        /// The id it is installed as.
        lab_id: String,
        /// The folder the player picked.
        src: PathBuf,
    },
    /// Bring a parked lab back (or the machines of a lab that went down).
    Resume {
        /// The lab.
        lab_id: String,
        /// Its runtime.
        runtime: Runtime,
    },
    /// Pause or shut the lab down, machines kept.
    Park {
        /// The lab.
        lab_id: String,
        /// Its runtime.
        runtime: Runtime,
        /// Pause (state saved) or shut down (powered off).
        mode: Park,
    },
    /// Run the provisioners again on one machine (or all).
    Provision {
        /// The lab.
        lab_id: String,
        /// Its runtime.
        runtime: Runtime,
        /// The machine, or none for all of them.
        machine: Option<String>,
    },
    /// Stop the lab wherever it runs and remove what it ran, ending what still runs on it first.
    Stop {
        /// The lab.
        lab_id: String,
        /// Its runtime.
        runtime: Runtime,
    },
    /// Add the attack box to the running lab.
    AttachBox {
        /// The lab.
        lab_id: String,
        /// Its runtime.
        runtime: Runtime,
        /// Where it joins, and with which image.
        pick: AttachPick,
    },
    /// Take the attack box out of the running lab.
    DetachBox {
        /// The lab.
        lab_id: String,
        /// Its runtime.
        runtime: Runtime,
    },
}

impl Request {
    /// The lab it acts on.
    pub(crate) fn lab_id(&self) -> &str {
        match self {
            Request::Launch(r) => &r.lab_id,
            Request::LaunchLocal { lab_id, .. }
            | Request::Resume { lab_id, .. }
            | Request::Park { lab_id, .. }
            | Request::Provision { lab_id, .. }
            | Request::Stop { lab_id, .. }
            | Request::AttachBox { lab_id, .. }
            | Request::DetachBox { lab_id, .. } => lab_id,
        }
    }

    /// What it is, as the UI names it.
    pub(crate) fn operation(&self) -> Operation {
        match self {
            Request::Launch(_) | Request::LaunchLocal { .. } => Operation::Launch,
            Request::Resume { .. } => Operation::Resume,
            Request::Park { mode: Park::Pause, .. } => Operation::Pause,
            Request::Park { mode: Park::Shutdown, .. } => Operation::Shutdown,
            Request::Provision { .. } => Operation::Provision,
            Request::Stop { .. } => Operation::Stop,
            Request::AttachBox { .. } => Operation::Attach,
            Request::DetachBox { .. } => Operation::Detach,
        }
    }

    /// The machine it targets, when one.
    pub(crate) fn machine(&self) -> Option<String> {
        match self {
            Request::Provision { machine, .. } => machine.clone(),
            _ => None,
        }
    }

    /// The runtime the lab is operated in, when the request says (a launch's is in its spec).
    pub(crate) fn runtime(&self) -> Option<Runtime> {
        match self {
            Request::Launch(_) => None,
            Request::LaunchLocal { .. } => Some(Runtime::Docker),
            Request::Resume { runtime, .. }
            | Request::Park { runtime, .. }
            | Request::Provision { runtime, .. }
            | Request::Stop { runtime, .. }
            | Request::AttachBox { runtime, .. }
            | Request::DetachBox { runtime, .. } => Some(*runtime),
        }
    }

    /// Whether the UI shows the lab as deploying while it runs: everything but a stop and a park.
    pub(crate) fn deploying(&self) -> bool {
        !matches!(self, Request::Stop { .. } | Request::Park { .. })
    }
}

/// Runs the lab operations and tells of them by events.
pub(crate) trait RunService: Send + Sync {
    /// Starts `request`; its id once it is under way (its log and its end are events). Refused,
    /// before anything changed, while another operation acts on the lab (a stop excepted: it ends
    /// what runs on the lab first).
    ///
    /// # Errors
    ///
    /// The refusal.
    fn start(&self, request: Request) -> Result<OperationId>;

    /// The events of every operation.
    fn events(&self) -> &EventLog;

    /// Follows the home's runner service for as long as its stream lasts: the operations another
    /// process queued there (the command line, an earlier run of the app) told as events too,
    /// and the screens of a lab whose run leaves its standing state ended.
    fn follow_runner(&self) -> BoxFuture<'static, ()>;
}

/// How long the app waits before following the runner service again once its stream ended.
const FOLLOW_AGAIN: std::time::Duration = std::time::Duration::from_secs(5);

/// A boxed future, as a trait object's method returns one.
pub(crate) type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The app's run service: made on first use, the same for the rest of the app's life. A program
/// without the app (the command line) makes its own with [`LocalRunService::new`] over the same
/// folders: the same store, workers and lab locks, so it is safe beside the open app.
pub(crate) fn of(app: &AppHandle) -> Arc<dyn RunService> {
    if let Some(service) = app.try_state::<Arc<dyn RunService>>() {
        return Arc::clone(&service);
    }
    let made: Arc<dyn RunService> = match LocalRunService::of(&crate::platform::AppDirs::from(app)) {
        Ok(service) => service,
        // No data folder: a service of its own, whose operations fail on the folder they need.
        Err(_) => Arc::new(LocalRunService::new(crate::platform::AppDirs::from(app))),
    };
    app.manage(made);
    Arc::clone(&app.state::<Arc<dyn RunService>>())
}

/// Runs `request` on the app's run service to its end: for what waits for an operation (the
/// Machine page's stop), not for the webview, which follows the events.
///
/// # Errors
///
/// The refusal, or the operation's failure.
pub(crate) async fn run(app: &AppHandle, request: Request) -> Result<()> {
    let service = of(app);
    let id = service.start(request)?;
    service.events().ended(&id).await.map_err(Error::from)
}

/// Connects the app's run service to the webview (every event, as [`RUN_EVENT`]) and picks up the
/// operations an earlier run of the app left running.
pub(crate) fn start(app: &AppHandle) {
    let service = of(app);
    let to = app.clone();
    service.events().listen(Box::new(move |e| crate::app::events::emit(&to, RUN_EVENT, e.clone())));
    // What another process asks of the runner service (an earlier run of the app, the command
    // line), told as it happens; followed again whenever the service's stream ends.
    tauri::async_runtime::spawn(async move {
        loop {
            service.follow_runner().await;
            tokio::time::sleep(FOLLOW_AGAIN).await;
        }
    });
}
