//! The auto-stop reaper: destroys cloud labs whose auto-stop time has passed, to end billing. On
//! Azure an OS poweroff only stops (not deallocates) the VM, so it keeps billing until a
//! `destroy`, and a destroy also cleans up leftover resources on every cloud. Cloud labs run
//! through the Isothrum runner now, which keeps each run's state, account and expiry; this sweep
//! asks the runner to destroy the ones past their stop time, with the account each was created
//! on. Runs on startup and periodically while the app is open; a lab that expired while the app
//! was closed is reaped at the next sweep.
//!
//! A reap that fails is never silent: it is logged, kept for the lab's page (a per-lab
//! `reap-error` record, shown while the lab is past its stop time) and notified once per lab per
//! sweep it newly fails on. A failure that never recovers on its own (expired credentials, a
//! deleted API key) would otherwise leave the lab billing unnoticed.
//!
//! Labs a player started on the launcher's own Terraform path before the runner existed are
//! handed to the runner first ([`hand_to_runner`]), so one code path — the runner's `down` and
//! this sweep — destroys every cloud lab.

use std::collections::BTreeSet;
use std::path::Path;

use crate::AppHandle;
use tauri::Manager;

use super::inflight::try_lock_lab;
use super::providers::Provider;
use super::{isothrum, server};
use crate::error::Wire;

/// Folder under the app data dir: one reap-error record per lab id the last sweep couldn't
/// destroy (`{message, error}`). A later sweep that destroys the lab clears it.
const REAP_ERRORS: &str = "reap-errors";

/// Destroys every cloud lab the runner records as past its auto-stop (see the module), handing the
/// launcher's own pre-runner deployments to the runner first.
pub async fn reap_expired_labs(app: &AppHandle) {
    hand_to_runner(app).await;
    reap_runner_clouds(app).await;
}

/// Destroys the cloud labs (cloud-vm, cloud-docker, cloud-services) the runner records as past
/// their auto-stop, with the account each was created on. The runner keeps their state and expiry;
/// this ends what they cost. A failure is logged, kept for the lab's page and notified once; a
/// lab destroyed (or no longer expired) has its record cleared.
async fn reap_runner_clouds(app: &AppHandle) {
    let Ok(home) = super::paths::runner_home(app) else { return };
    let Ok(base) = app.path().app_data_dir() else { return };
    let account = |host: &str| server::connection(app, host).ok().map(|c| c.tf_env);
    let failures = isothrum::reap_expired_clouds(&home, std::time::SystemTime::now(), account).await;
    // The labs that failed last time, read before clearing: a lab that fails again isn't
    // re-notified, and a lab destroyed this time (no new record) has its record go.
    let dir = base.join(REAP_ERRORS);
    let prior = failed_last_time(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    for (run, e) in failures {
        let id = run.strip_prefix("cyberctf-").unwrap_or(&run).to_string();
        let message = format!("Auto-stop couldn't destroy {id}: {e}");
        log::warn!("{message}");
        if let Err(w) = record_failure(&dir, &id, &e.to_wire(), &message) {
            log::warn!("auto-stop: couldn't record the failure for {id}: {w}");
        }
        if !prior.contains(&id) {
            notify(app, "Auto-stop failed", &message);
        }
    }
}

/// The lab ids that had a reap-error record (the last sweep couldn't destroy them).
fn failed_last_time(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect()
}

/// Keeps a failed reap of lab `id` for its page (`{message, error}`), under the reap-errors folder.
fn record_failure(dir: &Path, id: &str, error: &Wire, message: &str) -> std::io::Result<()> {
    if !valid_id(id) {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let record = serde_json::json!({ "message": message, "error": error });
    std::fs::write(dir.join(id), record.to_string())
}

/// A lab id is a plain name (a file name under the reap-errors folder).
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Why the auto-stop couldn't destroy cloud lab `id` the last time it tried, while the lab is
/// still billing: the lab page shows it, so a lab billing past its auto-stop never looks stopped.
pub fn last_failure(app: &AppHandle, id: &str) -> Option<Wire> {
    if !valid_id(id) {
        return None;
    }
    let base = app.path().app_data_dir().ok()?;
    let record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(base.join(REAP_ERRORS).join(id)).ok()?).ok()?;
    serde_json::from_value(record["error"].clone()).ok()
}

/// Hands the labs a player started on the launcher's own Terraform path (before the runner) to the
/// Isothrum runner ([`isothrum::import_all`]): at the start of every sweep (so at app start, and
/// before the runner's reaper looks). A lab that can't be handed over stays the launcher's,
/// stopped and reaped as before, and the player is told why once per new reason.
pub(super) async fn hand_to_runner(app: &AppHandle) {
    let Ok(base) = app.path().app_data_dir() else { return };
    let runner = match isothrum::import_runner(&base) {
        Ok(runner) => runner,
        Err(e) => {
            log::warn!("the launcher's own deployments can't be handed to the runner: {e}");
            return;
        }
    };
    let lock = |id: &str| try_lock_lab(app, id).ok().flatten();
    for (d, outcome) in isothrum::import_all(&runner, &base, super::launch::runner_runs_on, lock).await {
        match outcome {
            isothrum::Outcome::Imported => log::info!("lab {}'s deployment on {} is the Isothrum runner's now", d.id, d.provider.id()),
            isothrum::Outcome::Left(why) => log::debug!("lab {}'s deployment on {} stays as it is: {why:?}", d.id, d.provider.id()),
            isothrum::Outcome::Failed { error, new } => {
                let message = handover_message(&d.id, d.provider, &error);
                log::warn!("{message}");
                if new {
                    notify(app, "Lab engine", &message);
                }
            }
        }
    }
}

/// What the player is told about lab `id`'s deployment on `provider` the runner couldn't take.
fn handover_message(id: &str, provider: Provider, why: &impl std::fmt::Display) -> String {
    let place = if provider.is_cloud() { format!("your {} account", provider.id().to_uppercase()) } else { "your Proxmox server".into() };
    format!(
        "Lab {id}, running in {place}, couldn't be handed over to the Isothrum runner: {why}. It stays with the launcher's own engine until you stop it, so Stop and the auto-stop still remove it."
    )
}

/// A system notification titled `title`, when the app has the notification plugin (not in unit
/// tests).
fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if app.try_state::<tauri_plugin_notification::Notification<crate::AppRuntime>>().is_some() {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

#[cfg(test)]
mod tests;
