//! The values a lab's run on this machine was launched with (the launch token, the player's
//! flags, the API's URL), kept with the launcher's secrets (`server::secrets`: the system
//! keychain, under `launch-values:<run>`) for as long as the run lasts. The runner service holds a
//! run's secrets in memory only, and a lab whose input has no default needs its value at every
//! operation of its tool, Compose's `up --no-recreate` of a resume included: so a lab parked, the
//! app restarted, then resumed, is resumed with the values it was launched with. Every later
//! operation of the run gets them back as its start did ([`super::isothrum::Deployment`]), the
//! sensitive ones as secrets, never in a log, a file of the app's or the run's record.
//!
//! A start keeps them, replacing any kept before (a start with none forgets them); a run taken
//! down forgets them, as does the lab's removal and a sign-out (they are the account's). None kept
//! (a run started before the launcher kept them, a keychain that refused) is what a resume had
//! before: the lab's defaults.
//!
//! On macOS a keychain entry is one prompt at most for a build the keychain doesn't know: a run's
//! values are one entry, read once per run in a process and then held in memory.

use std::collections::BTreeMap;

use super::server::secrets;
use crate::error::Result;

/// A run's values, by name, in the order the launch gave them.
type Values = Vec<(String, String)>;

/// The store's name for run `run`'s values.
fn entry(run: &str) -> String {
    format!("launch-values:{run}")
}

/// The values kept or read in this process, by run: the keychain is asked once per run.
#[cfg(not(test))]
static HELD: std::sync::Mutex<BTreeMap<String, Values>> = std::sync::Mutex::new(BTreeMap::new());
#[cfg(test)]
thread_local! {
    /// Per test thread, like the secrets file a test keeps them in.
    static HELD: std::sync::Mutex<BTreeMap<String, Values>> = const { std::sync::Mutex::new(BTreeMap::new()) };
}

/// `f` on the values held in this process.
fn with_held<T>(f: impl FnOnce(&mut BTreeMap<String, Values>) -> T) -> T {
    let run = |m: &std::sync::Mutex<BTreeMap<String, Values>>| f(&mut m.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    #[cfg(test)]
    return HELD.with(run);
    #[cfg(not(test))]
    run(&HELD)
}

/// Keeps `values` as those run `run` was launched with, in place of any kept before; none (a lab
/// imported, started offline) forgets them. A store that refuses is logged, by the values' count
/// only: the start goes on, and a resume after a restart gets the lab's defaults.
pub(crate) fn keep(run: &str, values: &[(String, String)]) {
    if values.is_empty() {
        if let Err(e) = forget(run) {
            log::warn!("the values {run} was launched with before stay in the keychain: {e}");
        }
        return;
    }
    with_held(|held| held.insert(run.to_owned(), values.to_vec()));
    let saved = serde_json::to_string(values).map_err(|e| crate::error::Error::Invalid(e.to_string())).and_then(|raw| secrets::set_named(&entry(run), &raw));
    if let Err(e) = saved {
        log::warn!("the {} values {run} was launched with aren't kept, so a resume after a restart gets the lab's defaults: {e}", values.len());
    }
}

/// The values run `run` was launched with: none when none are kept, or the store can't say (said
/// in the log, never the values).
pub(crate) fn kept(run: &str) -> Values {
    if let Some(values) = with_held(|held| held.get(run).cloned()) {
        return values;
    }
    let values = match secrets::get_named(&entry(run)) {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_else(|e: serde_json::Error| {
            log::warn!("the values {run} was launched with aren't ones this version reads ({:?} at column {}): none given", e.classify(), e.column());
            Vec::new()
        }),
        Ok(None) => Vec::new(),
        Err(e) => {
            log::warn!("the values {run} was launched with can't be read: none given ({e})");
            return Vec::new();
        }
    };
    with_held(|held| held.insert(run.to_owned(), values.clone()));
    values
}

/// Forgets the values run `run` was launched with, here and in the store; none kept is forgotten
/// already.
///
/// # Errors
///
/// The store refused (a locked keychain): they stay there.
pub(crate) fn forget(run: &str) -> Result<()> {
    with_held(|held| held.remove(run));
    secrets::delete_named(&entry(run))
}

/// [`forget`] for lab `id`'s run (`cyberctf-<id>`).
pub(crate) fn forget_lab(id: &str) -> Result<()> {
    forget(&super::docker::project(id))
}

/// Forgets the values of every installed lab's run: a sign-out, the values being the account's.
/// One the store refuses is logged and the others are forgotten still. Blocking (the keychain's
/// calls hand the worker's other tasks on, `server::secrets`).
pub(crate) fn forget_all(dirs: &crate::platform::AppDirs) {
    with_held(BTreeMap::clear);
    let Ok(labs) = super::paths::labs_root(dirs) else { return };
    let Ok(entries) = std::fs::read_dir(labs) else { return };
    for id in entries.flatten().filter_map(|e| e.file_name().to_str().map(str::to_owned)).filter(|id| super::paths::is_lab_id(id)) {
        if let Err(e) = forget_lab(&id) {
            log::warn!("signing out: the values {id} was launched with stay in the keychain: {e}");
        }
    }
}

/// Lets go of the values held in this process, as an app restarting does: the next read asks the
/// store.
#[cfg(test)]
pub(crate) fn restart() {
    with_held(BTreeMap::clear);
}

#[cfg(test)]
mod tests;
