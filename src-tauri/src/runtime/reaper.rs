//! The auto-stop reaper: destroys cloud labs whose auto-stop time has passed, to end billing. On
//! Azure an OS poweroff only stops (not deallocates) the VM, so it keeps billing until a
//! `destroy`, and a destroy also cleans up leftover resources on every cloud. Runs on startup and
//! periodically while the app is open; a lab that expired while the app was closed is reaped at
//! the next sweep. Every state of a lab counts, not only the one its marker points at: a state
//! left on a host the lab no longer runs on is destroyed with the host it was applied on.
//!
//! A reap that fails is never silent: it is logged, kept beside the state (`reap-error`, shown on
//! the lab's page while the lab is past its stop time) and notified once per new failure. A
//! failure that never recovers on its own (expired credentials, a deleted API key) would
//! otherwise leave the lab billing unnoticed, the sweep failing the same way every 5 minutes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::AppHandle;
use tauri::Manager;

use super::inflight::try_lock_lab;
use super::lifecycle::stop_locked;
use super::paths::{deployed_ids, deployment_states, lab_dir, recorded_host, recorded_runtime};
use super::{Runtime, cloud_services, lab, server, terraform};
use crate::error::{Error, Result};

/// Beside a state: the last reap that failed on it (JSON: the stop time it was reaping, why).
const REAP_ERROR: &str = "reap-error";

/// How long one lab's reap may take before it is given up and the sweep moves on to the next
/// lab: above Terraform's own destroy limit, which says more when it is the one that ran out.
const REAP_TIMEOUT: Duration = Duration::from_secs(35 * 60);

/// Why an expired deployment couldn't be destroyed.
#[derive(Debug)]
enum ReapFailure {
    /// Nothing says which account's credentials can destroy it (a state from before hosts were
    /// recorded, its lab moved elsewhere).
    NoHost,
    /// The lab isn't installed any more: its Terraform module is gone with it.
    LabGone(Error),
    /// The account it was created with can't be used (removed, or its secret is unreadable).
    Account(String, Error),
    /// The destroy itself failed (expired credentials, a cloud error).
    Destroy(String, Error),
    /// The reap ran past [`REAP_TIMEOUT`].
    TimedOut,
}

impl std::fmt::Display for ReapFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReapFailure::NoHost => f.write_str("no account is recorded for it, so the launcher can't destroy it. Remove its resources in your cloud console."),
            ReapFailure::LabGone(e) => {
                write!(f, "its lab files are gone ({e}). Install the lab again, then stop it, or remove its resources in your cloud console.")
            }
            ReapFailure::Account(host, e) => write!(f, "the account {host} can't be used ({e}). Fix the account in Settings, then stop the lab."),
            ReapFailure::Destroy(host, e) => write!(f, "destroying it on {host} failed: {e}"),
            ReapFailure::TimedOut => write!(f, "the destroy didn't finish within {} minutes. Stop the lab to retry.", REAP_TIMEOUT.as_secs() / 60),
        }
    }
}

impl ReapFailure {
    /// What kind of failure it is, and where: the same again (a sweep failing the same way every
    /// 5 minutes, its tool's message differing only by a request id or a time) is not new.
    fn kind(&self) -> String {
        match self {
            ReapFailure::NoHost => "no-host".into(),
            ReapFailure::LabGone(_) => "lab-gone".into(),
            ReapFailure::Account(host, _) => format!("account:{host}"),
            ReapFailure::Destroy(host, _) => format!("destroy:{host}"),
            ReapFailure::TimedOut => "timed-out".into(),
        }
    }
}

/// What a reap did when nothing failed.
#[derive(Debug, PartialEq)]
enum Reaped {
    /// Destroyed.
    Destroyed,
    /// Left for the next sweep: the lab is busy with another operation, or no longer expired
    /// (restarted or extended meanwhile).
    Skipped,
}

/// Destroys every expired deployment (see the module). A failure is recorded and surfaced; the
/// next sweep retries it.
pub async fn reap_expired_labs(app: &AppHandle) {
    let Ok(base) = app.path().app_data_dir() else {
        log::warn!("auto-stop: no app data folder, so no expired lab can be found");
        return;
    };
    for (id, target, state) in expired_states(&base) {
        match within(REAP_TIMEOUT, reap(app, &id, &target, &state)).await {
            Ok(Reaped::Destroyed) => forget_failure(&state),
            Ok(Reaped::Skipped) => {}
            Err(failure) => {
                if record_failure(&id, &state, &failure) {
                    notify(app, &failure_message(&id, &failure));
                }
            }
        }
    }
}

/// `reap`, given up as [`ReapFailure::TimedOut`] after `limit` (and so killed, with the tools
/// it was running), so one hung lab never keeps the others billing.
async fn within(limit: Duration, reap: impl Future<Output = std::result::Result<Reaped, ReapFailure>>) -> std::result::Result<Reaped, ReapFailure> {
    tokio::time::timeout(limit, reap).await.unwrap_or(Err(ReapFailure::TimedOut))
}

/// Destroys the expired deployment of lab `id` on `target` (folder `state`), with the account it
/// was created on.
async fn reap(app: &AppHandle, id: &str, target: &str, state: &Path) -> std::result::Result<Reaped, ReapFailure> {
    let dir = lab_dir(app, id).map_err(ReapFailure::LabGone)?;
    let marked = server::marked_host(&dir).and_then(|h| Some((server::terraform_target(server::host_provider(app, &h)?)?, h)));
    let owner = reap_owner(marked.as_ref().map(|(tf, h)| (*tf, h.as_str())), recorded_host(state).as_deref(), target).ok_or(ReapFailure::NoHost)?;
    // Take the lab lock before tearing it down, and re-check expiry while holding it: between
    // the cheap filter of the sweep and here a user may have restarted or extended this lab (a
    // start re-applies and writes a fresh expiry), and destroying their live lab would be wrong.
    // A lab busy with another operation (a deploy worker's, maybe for hours) is left to the
    // next sweep rather than holding up the others.
    let _lock = match try_lock_lab(app, id) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Ok(Reaped::Skipped),
        // The lock can't be had at all (its file): tried again at the next sweep.
        Err(e) => {
            log::warn!("auto-stop: not reaping {id} this time: {e}");
            return Ok(Reaped::Skipped);
        }
    };
    if !terraform::expired(state) {
        return Ok(Reaped::Skipped);
    }
    // Torn down with the runtime recorded at start. No UI context here, so the log is dropped;
    // a failure is what `reap_expired_labs` records.
    let runtime = recorded_runtime(state);
    match owner {
        Owner::Current => {
            let host = marked.map(|(_, h)| h).unwrap_or_default();
            stop_locked(app, &dir, id, runtime, |_line: String| {}).await.map_err(|e| ReapFailure::Destroy(account_name(app, &host), e))?;
        }
        // Left behind: destroyed on its own host; the lab's markers (where it runs now) stay.
        Owner::Left(host) => {
            let c = server::connection(app, &host).map_err(|e| ReapFailure::Account(account_name(app, &host), e))?;
            destroy_left(app, &dir, id, runtime, (target, state), &c).await.map_err(|e| ReapFailure::Destroy(c.name.clone(), e))?;
        }
    }
    Ok(Reaped::Destroyed)
}

/// An account's name for the player, else its id.
fn account_name(app: &AppHandle, host: &str) -> String {
    server::host_name(app, host).unwrap_or_else(|| host.to_string())
}

/// Destroys a deployment of lab `id` left on host `c` (its Terraform `target` and `state`), with
/// the module that created it: the lab's own for cloud services, else its Isoloom module.
async fn destroy_left(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, (target, state): (&str, &Path), c: &server::Connection) -> Result<()> {
    if runtime == Runtime::Cloud {
        return cloud_services::destroy(app, dir, id, c, |_line: String| {}).await;
    }
    let module = lab::terraform_to_destroy(dir, runtime, target)?;
    terraform::destroy(&module, state, &c.tf_vars, &c.tf_env, |_line: String| {}).await
}

/// Every expired Terraform state under the app data folder `base`: lab id, target, folder.
fn expired_states(base: &Path) -> Vec<(String, String, PathBuf)> {
    deployed_ids(base)
        .into_iter()
        .flat_map(|id| deployment_states(base, &id).into_iter().map(move |(target, state)| (id.clone(), target, state)))
        .filter(|(_, _, state)| terraform::expired(state))
        .collect()
}

/// Whose an expired state is.
#[derive(Debug, PartialEq)]
enum Owner {
    /// The lab's current deployment (its marker names a host of this target).
    Current,
    /// Left on this host, which the lab no longer runs on.
    Left(String),
}

/// Whose the expired state of `target` is: the current deployment's when the lab is marked
/// (`marked`: the marked host's target and id) as running on a host of that target, else the
/// leftover of the host recorded with the state (`recorded`). `None` when no host is known
/// (a state from before hosts were recorded).
fn reap_owner(marked: Option<(&str, &str)>, recorded: Option<&str>, target: &str) -> Option<Owner> {
    match (marked, recorded) {
        (Some((tf, host)), recorded) if tf == target && recorded.is_none_or(|r| r == host) => Some(Owner::Current),
        (_, Some(recorded)) => Some(Owner::Left(recorded.to_string())),
        _ => None,
    }
}

/// What the player is told about a failed reap of lab `id`.
fn failure_message(id: &str, failure: &ReapFailure) -> String {
    format!("Auto-stop couldn't destroy {id}: {failure}")
}

/// A failed reap: logged, and kept beside the state for the lab's page. Whether it is new, to be
/// notified: a failure of another kind than the last one recorded for this stop time.
fn record_failure(id: &str, state: &Path, failure: &ReapFailure) -> bool {
    let message = failure_message(id, failure);
    log::warn!("{message}");
    let expires_at = terraform::expires_at(state);
    let new = recorded(state, expires_at).is_none_or(|r| r["kind"].as_str() != Some(failure.kind().as_str()));
    let record = serde_json::json!({ "expires_at": expires_at, "kind": failure.kind(), "message": message });
    if let Err(e) = std::fs::write(state.join(REAP_ERROR), record.to_string()) {
        log::warn!("auto-stop: couldn't record the failure beside {}: {e}", state.display());
    }
    new
}

/// Clears what an earlier failed reap of this state recorded.
fn forget_failure(state: &Path) {
    let _ = std::fs::remove_file(state.join(REAP_ERROR));
}

/// The failure recorded beside `state` for the stop time `expires_at`: a later launch (a new
/// stop time) or an older record never shows.
fn last_failure_at(state: &Path, expires_at: Option<u64>) -> Option<String> {
    recorded(state, expires_at)?["message"].as_str().map(str::to_string)
}

/// The failure record beside `state`, when it is about the stop time `expires_at`.
fn recorded(state: &Path, expires_at: Option<u64>) -> Option<serde_json::Value> {
    let record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(state.join(REAP_ERROR)).ok()?).ok()?;
    (expires_at.is_some() && record["expires_at"].as_u64() == expires_at).then_some(record)
}

/// Why the auto-stop couldn't destroy lab `id` (on any of its deployments), while it is still
/// past its stop time: the lab page shows it, so a lab billing past its auto-stop never looks
/// stopped.
pub fn last_failure(app: &AppHandle, id: &str) -> Option<String> {
    let base = app.path().app_data_dir().ok()?;
    deployment_states(&base, id)
        .into_iter()
        .find_map(|(_, state)| last_failure_at(&state, terraform::expires_at(&state)).filter(|_| terraform::expired(&state)))
}

/// A system notification, when the app has the notification plugin (not in unit tests).
fn notify(app: &AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if app.try_state::<tauri_plugin_notification::Notification<crate::AppRuntime>>().is_some() {
        let _ = app.notification().builder().title("Auto-stop failed").body(body).show();
    }
}

#[cfg(test)]
mod tests;
