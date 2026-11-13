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
//! this sweep — destroys every cloud lab. The launcher no longer runs Terraform itself: a
//! deployment the runner can't take is removed by nothing here, so the player is told to remove
//! it by hand ([`handover_message`]), and the hand-over is tried again at every sweep.

use std::collections::BTreeSet;
use std::path::Path;

use super::inflight::try_lock_lab;
use super::providers::Provider;
use super::{isothrum, server};
use crate::error::Wire;
use crate::platform::AppDirs;

/// How the reaper tells the player a lab needs attention (a failed auto-stop, a deployment the
/// runner couldn't take). The app shell supplies a system notification; unit tests a no-op. This
/// is the only thing the reaper reaches the UI for, so it stays domain and tauri-free.
pub type Report = dyn Fn(&str, &str) + Send + Sync;

/// Folder under the app data dir: one reap-error record per lab id the last sweep couldn't
/// destroy (`{message, error}`). A later sweep that destroys the lab clears it.
const REAP_ERRORS: &str = "reap-errors";

/// Destroys every cloud lab the runner records as past its auto-stop (see the module), handing the
/// launcher's own pre-runner deployments to the runner first.
pub async fn reap_expired_labs(dirs: &AppDirs, report: &Report) {
    hand_to_runner(dirs, report).await;
    reap_runner_clouds(dirs, report).await;
}

/// Destroys the cloud labs (cloud-vm, cloud-docker, cloud-services) the runner records as past
/// their auto-stop, with the account each was created on. The runner keeps their state and expiry;
/// this ends what they cost. A failure is logged, kept for the lab's page and notified once; a
/// lab destroyed (or no longer expired) has its record cleared.
async fn reap_runner_clouds(dirs: &AppDirs, report: &Report) {
    let Ok(home) = super::paths::runner_home(dirs) else { return };
    let Ok(base) = dirs.data() else { return };
    let account = |host: &str| server::connection(dirs, host).ok().map(|c| c.tf_env);
    // Each destroy under the lab's per-lab lock, like `hand_to_runner`: a user Stop or install of
    // the lab can't race the reaper's Terraform destroy over the same state. A busy lab is left
    // for a later sweep (inside `reap_expired`).
    let lock = |id: &str| try_lock_lab(dirs, id).ok().flatten();
    let failures = isothrum::reap_expired_clouds(&home, std::time::SystemTime::now(), account, lock).await;
    // The labs that failed last time, read before clearing: a lab that fails again isn't
    // re-notified, and a lab destroyed this time (no new record) has its record go.
    let dir = base.join(REAP_ERRORS);
    let prior = failed_last_time(&dir);
    // Not cleared, a destroyed lab's record would go on saying it couldn't be.
    if let Err(e) = crate::platform::fs::remove_dir_if_present(&dir) {
        log::warn!("auto-stop: couldn't clear the last sweep's failures in {}: {e}", dir.display());
    }
    for (run, e) in failures {
        let id = super::docker::lab_of_project(&run).unwrap_or(&run).to_string();
        let message = format!("Auto-stop couldn't destroy {id}: {e}");
        log::warn!("{message}");
        if let Err(w) = record_failure(&dir, &id, &e.to_wire(), &message) {
            log::warn!("auto-stop: couldn't record the failure for {id}: {w}");
        }
        if !prior.contains(&id) {
            report("Auto-stop failed", &message);
        }
    }
}

/// The lab ids that had a reap-error record (the last sweep couldn't destroy them).
fn failed_last_time(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect()
}

/// Keeps a failed reap of lab `id` for its page (`{message, error}`), under the reap-errors folder.
fn record_failure(dir: &Path, id: &str, error: &Wire, message: &str) -> std::io::Result<()> {
    if !super::is_lab_id(id) {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let record = serde_json::json!({ "message": message, "error": error });
    std::fs::write(dir.join(id), record.to_string())
}

/// Why the auto-stop couldn't destroy cloud lab `id` the last time it tried, while the lab is
/// still billing: the lab page shows it, so a lab billing past its auto-stop never looks stopped.
pub fn last_failure(dirs: &AppDirs, id: &str) -> Option<Wire> {
    if !super::is_lab_id(id) {
        return None;
    }
    let base = dirs.data().ok()?;
    let path = base.join(REAP_ERRORS).join(id);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        // A record that is there but can't be read is a failure to say, not "no failure"
        // (Clean Code ch.7, 2026-11-13).
        Err(e) => {
            log::warn!("auto-stop: the last failure of {id} couldn't be read from {}: {e}", path.display());
            return None;
        }
    };
    let record: serde_json::Value = match serde_json::from_str(&text) {
        Ok(record) => record,
        Err(e) => {
            log::warn!("auto-stop: the last failure of {id} in {} isn't JSON: {e}", path.display());
            return None;
        }
    };
    serde_json::from_value(record["error"].clone()).ok()
}

/// Hands the labs a player started on the launcher's own Terraform path (before the runner) to the
/// Isothrum runner ([`isothrum::import_all`]): at the start of every sweep (so at app start, and
/// before the runner's reaper looks). A lab that can't be handed over stays where it is, which
/// neither Stop nor the auto-stop can remove until a later sweep hands it over: the player is
/// told why, and how to remove it by hand, once per new reason.
pub(super) async fn hand_to_runner(dirs: &AppDirs, report: &Report) {
    let Ok(base) = dirs.data() else { return };
    let runner = match isothrum::import_runner(base) {
        Ok(runner) => runner,
        Err(e) => {
            log::warn!("the launcher's own deployments can't be handed to the runner: {e}");
            return;
        }
    };
    let lock = |id: &str| try_lock_lab(dirs, id).ok().flatten();
    for (d, outcome) in isothrum::import_all(&runner, base, super::launch::runner_runs_on, lock).await {
        match outcome {
            isothrum::Outcome::Imported => log::info!("lab {}'s deployment on {} is the Isothrum runner's now", d.id, d.provider.id()),
            isothrum::Outcome::Left(why) => log::debug!("lab {}'s deployment on {} stays as it is: {why:?}", d.id, d.provider.id()),
            isothrum::Outcome::Failed { error, new } => {
                let message = handover_message(&d.id, d.provider, &error);
                log::warn!("{message}");
                if new {
                    report("Lab engine", &message);
                }
            }
        }
    }
}

/// What the player is told about lab `id`'s deployment on `provider` the runner couldn't take.
fn handover_message(id: &str, provider: Provider, why: &impl std::fmt::Display) -> String {
    let place = if provider.is_cloud() { format!("your {} account", provider.label()) } else { "your Proxmox server".into() };
    let costs = if provider.is_cloud() { ", and it keeps costing" } else { "" };
    format!(
        "Lab {id}, running in {place}, couldn't be handed over to the Isothrum runner: {why}. Until it is, the launcher can't remove it: Stop and the auto-stop leave it running{costs}. {} {RETRIED}",
        remove_by_hand(provider)
    )
}

/// How the hand-over of a deployment the runner couldn't take is tried again.
pub(super) const RETRIED: &str = "The hand-over is tried again every few minutes while the app is open.";

/// What the player does to remove by hand a deployment on `provider` the launcher can't remove.
pub(super) fn remove_by_hand(provider: Provider) -> String {
    if provider.is_cloud() {
        format!("To stop it now, delete its resources in your {} console.", provider.label())
    } else {
        "To stop it now, delete its VMs on the Proxmox server.".into()
    }
}

#[cfg(test)]
mod tests;
