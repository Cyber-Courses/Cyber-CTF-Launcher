//! The app's run service ([`crate::run_service`]) inside the Tauri app: kept in the app's state,
//! its events sent to every window, the runner service followed and the watched labs reconciled
//! for as long as the app runs.

use std::sync::Arc;

use tauri::Manager;

use crate::AppHandle;
use crate::error::{Error, Result};
use crate::run_service::{LocalRunService, RECONCILE_EVERY, RUN_EVENT, Request, RunService};

/// How long the app waits before following the runner service again once its stream ended.
const FOLLOW_AGAIN: std::time::Duration = std::time::Duration::from_secs(5);

/// The app's run service: made on first use, the same for the rest of the app's life. A program
/// without the app (the command line) makes its own with [`LocalRunService::new`] over the same
/// folders: the same store and the same runner service, which refuses a second operation on a
/// lab's run, so it is safe beside the open app.
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
    let following = Arc::clone(&service);
    tauri::async_runtime::spawn(async move {
        loop {
            following.follow_runner().await;
            tokio::time::sleep(FOLLOW_AGAIN).await;
        }
    });
    // The watched labs' statuses, reconciled with what runs.
    tauri::async_runtime::spawn(async move {
        loop {
            service.reconcile_once().await;
            tokio::time::sleep(RECONCILE_EVERY).await;
        }
    });
}
