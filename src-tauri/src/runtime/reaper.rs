//! The auto-stop reaper: destroys cloud labs whose auto-stop time has passed, to end billing. On
//! Azure an OS poweroff only stops (not deallocates) the VM, so it keeps billing until a
//! `destroy`, and a destroy also cleans up leftover resources on every cloud. Cloud labs run
//! through the Isothrum runner now, which keeps each run's state, account and expiry; this sweep
//! asks the runner to destroy the ones past their stop time, with the account each was created
//! on. Runs on startup and periodically while the app is open; a lab that expired while the app
//! was closed is reaped at the next sweep.
//!
//! A reap that fails is never silent: it is logged, kept for the lab's page (the launcher's store,
//! `crate::store::autostop`, shown while the lab is past its stop time) and notified once per lab per
//! sweep it newly fails on. A failure that never recovers on its own (expired credentials, a
//! deleted API key) would otherwise leave the lab billing unnoticed.

use std::collections::BTreeSet;

use super::{isothrum, server};
use std::path::Path;

use crate::error::{Code, Error, Result, Wire};
use crate::platform::AppDirs;

/// How the reaper tells the player a lab needs attention (a failed auto-stop). The app shell supplies a system notification; unit tests a no-op. This
/// is the only thing the reaper reaches the UI for, so it stays domain and tauri-free.
pub type Report = dyn Fn(&str, &str) + Send + Sync;

/// Destroys every cloud lab the runner records as past its auto-stop (see the module).
pub async fn reap_expired_labs(dirs: &AppDirs, report: &Report) {
    reap_runner_clouds(dirs, report).await;
}

/// Destroys the cloud labs (cloud-vm, cloud-docker, cloud-services) the runner records as past
/// their auto-stop, with the account each was created on. The runner keeps their state and expiry;
/// this ends what they cost. A failure is logged, kept for the lab's page and notified once; a
/// lab destroyed (or no longer expired) has its record cleared.
async fn reap_runner_clouds(dirs: &AppDirs, report: &Report) {
    let Ok(home) = super::paths::runner_home(dirs) else { return };
    // A connection that can't be read (the keychain refused, a server removed) leaves the lab
    // for a later sweep, said: silent, auto-stop would look as if it never ran.
    let account = |host: &str| match server::connection(dirs, host) {
        Ok(c) => Some(c.tf_env),
        Err(e) => {
            log::warn!("auto-stop: no account for the labs on {host}, left for a later sweep: {e}");
            None
        }
    };
    // A lab an operation of this process acts on (a Stop, an install) is left for a later sweep;
    // the runner service refuses the destroy itself while another process's runs on the run.
    let service = crate::run_service::LocalRunService::of(dirs).ok();
    let busy = |id: &str| service.as_ref().is_some_and(|s| s.is_busy(id));
    let failures = match sweep(&home, account, busy).await {
        Ok(failures) => failures,
        Err(e) => {
            // No sweep happened: said once, as the runner's, not as a lab that couldn't be destroyed.
            let message = format!("Auto-stop couldn't list the labs: {e}");
            log::warn!("{message}");
            report("Auto-stop failed", &message);
            return;
        }
    };
    keep_failures(dirs, failures, report);
}

/// Destroys every cloud run the runner service lists as past its expiry, with the account's
/// credentials resolved by `account` (a recorded account id to its Terraform environment), but
/// those of a lab `busy` says an operation of this process acts on. Returns the runs it couldn't
/// destroy, with why; a run whose account can't be resolved, whose lab is busy, or whose destroy
/// the runner service refuses because another operation holds the run, is left for a later
/// sweep. `Err` when the runs can't even be listed: no sweep happened.
async fn sweep<F, B>(home: &Path, account: F, busy: B) -> Result<Vec<(String, Error)>>
where
    F: Fn(&str) -> Option<Vec<(String, String)>>,
    B: Fn(&str) -> bool,
{
    let mut failed = Vec::new();
    for record in isothrum::expired_clouds(home).await? {
        let name = record.name.to_string();
        let id = super::docker::lab_of_project(&name).unwrap_or(&name).to_owned();
        // Busy: another operation acts on the lab, so it is left for a later sweep rather than
        // destroyed under it (not a failure).
        if busy(&id) {
            log::debug!("auto-stop: {name} is busy (an operation acts on its lab); left for a later sweep");
            continue;
        }
        let Some(host) = record.host_label.as_deref() else {
            failed.push((name, Error::coded(Code::ReapNoHost)));
            continue;
        };
        // The account removed or unreadable: left for a later sweep (it may come back), said.
        let Some(tf_env) = account(host) else {
            log::warn!("auto-stop: {name} is past its stop time, but its account {host} can't be used now; left for a later sweep");
            continue;
        };
        if let Err(e) = super::paths::lab_dir_in(home, &id) {
            log::warn!("auto-stop: {name}'s lab isn't installed any more ({e}); destroying it with the module its run kept");
        }
        match isothrum::destroy_expired(home, &record, &tf_env).await {
            Err(e) if e.code() == Code::AlreadyInProgress => log::debug!("auto-stop: {name} is busy in the runner service; left for a later sweep"),
            Err(e) => failed.push((name, e)),
            Ok(()) => {}
        }
    }
    Ok(failed)
}

/// Keeps `failures` (run, why) for the labs' pages, replacing the last sweep's, and notifies each
/// lab that newly failed.
fn keep_failures(dirs: &AppDirs, failures: Vec<(String, Error)>, report: &Report) {
    // The labs that failed last time, read before replacing them: a lab that fails again isn't
    // re-notified, and a lab destroyed this time (no new failure) has its record go.
    let store = match crate::store::open(dirs) {
        Ok(store) => store,
        Err(e) => {
            log::warn!("auto-stop: the last sweep's failures can't be kept: {e}");
            return;
        }
    };
    let prior: BTreeSet<String> = store.read(crate::store::autostop::failed).unwrap_or_default().into_iter().collect();
    let mut kept = Vec::new();
    for (run, e) in failures {
        let id = super::docker::lab_of_project(&run).unwrap_or(&run).to_string();
        let message = format!("Auto-stop couldn't destroy {id}: {e}");
        log::warn!("{message}");
        if !prior.contains(&id) {
            report("Auto-stop failed", &message);
        }
        if super::is_lab_id(&id) {
            kept.push((id, message, e.to_wire()));
        }
    }
    // Not replaced, a destroyed lab's record would go on saying it couldn't be.
    if let Err(e) = store.write(|tx| crate::store::autostop::replace(tx, &kept)) {
        log::warn!("auto-stop: couldn't keep this sweep's failures: {e}");
    }
}

/// Why the auto-stop couldn't destroy cloud lab `id` the last time it tried, while the lab is
/// still billing: the lab page shows it, so a lab billing past its auto-stop never looks stopped.
pub fn last_failure(dirs: &AppDirs, id: &str) -> Option<Wire> {
    if !super::is_lab_id(id) {
        return None;
    }
    // A record that is there but can't be read is a failure to say, not "no failure".
    crate::store::open(dirs).and_then(|store| store.read(|conn| crate::store::autostop::last(conn, id))).unwrap_or_else(|e| {
        log::warn!("auto-stop: the last failure of {id} couldn't be read: {e}");
        None
    })
}

#[cfg(test)]
mod tests;
