//! The rules of the saved hosts: which one is the default, what a host may change while labs are
//! on it, and how many labs are on each (marked as running there, or something of theirs left
//! there).

use std::collections::HashMap;
use std::path::Path;

use super::contract::marked_host;
use super::profile::HostProfile;
use super::store::{Store, find};
use crate::error::{Code, Error, Result};
use crate::platform::AppDirs;

/// Refuses to move a host (another provider, region or address, port, node, or the account labs
/// are created in) while `deployed` labs still have something on it: a stop would then look for
/// them where they aren't, find nothing, and leave them running (and, on a cloud, billing).
/// Everything else (its name, credentials, size, auto-stop, budget) can change.
pub(crate) fn keeps_placement(saved: &HostProfile, edited: &HostProfile, deployed: usize) -> Result<()> {
    let (before, after) = (saved.placement(), edited.placement());
    let mut changed: Vec<&str> = Vec::new();
    for (name, _) in before.iter().filter(|f| !after.contains(f)).chain(after.iter().filter(|f| !before.contains(f))) {
        if !changed.contains(name) {
            changed.push(name);
        }
    }
    if deployed == 0 || changed.is_empty() {
        return Ok(());
    }
    let (count, fields) = (deployed as u64, changed.join(", "));
    Err(Error::coded(if saved.provider.is_cloud() { Code::PlacementLockedAccount { count, fields } } else { Code::PlacementLockedHost { count, fields } }))
}

/// Adds or replaces a host; the first server saved becomes the default.
pub(crate) fn put_host(store: &mut Store, profile: HostProfile) {
    let id = profile.id.clone();
    let server = !profile.provider.is_cloud();
    match store.hosts.iter_mut().find(|h| h.id == id) {
        Some(existing) => *existing = profile,
        None => store.hosts.push(profile),
    }
    if store.default.is_none() && server {
        store.default = Some(id);
    }
}

/// Drops a host; when it was the default, another server (never a cloud account) takes over.
pub(crate) fn remove_host(store: &mut Store, id: &str) {
    store.hosts.retain(|h| h.id != id);
    if store.default.as_deref() == Some(id) {
        // The default is a *server* to run VM labs on; a cloud account must never become it.
        store.default = store.hosts.iter().find(|h| !h.provider.is_cloud()).map(|h| h.id.clone());
    }
}

/// Sets (or clears, with None) the host VM labs run on by default: a saved server, never a cloud
/// account.
pub(crate) fn set_default_host(store: &mut Store, id: Option<String>) -> Result<()> {
    if let Some(id) = &id
        && find(store, id)?.provider.is_cloud()
    {
        return Err(Error::Invalid("a cloud account can't be the default host".into()));
    }
    store.default = id;
    Ok(())
}

/// Refuses to remove a host `running` labs still run on.
pub(crate) fn no_labs_running(running: usize) -> Result<()> {
    if running > 0 {
        return Err(Error::coded(Code::HostHasLabs { count: running as u64 }));
    }
    Ok(())
}

/// How many labs still have something on host `id`: marked as running there, or left there by a
/// lab no longer marked for it ([`leftovers_on`]).
pub(crate) fn labs_on(dirs: &AppDirs, id: &str) -> Result<usize> {
    let marked = lab_markers(dirs).iter().filter(|m| *m == id).count();
    Ok(marked + leftovers_on(dirs, id)?)
}

/// How many installed labs are marked as running on each host, by host id (blank markers aren't
/// counted).
pub(crate) fn running_by_host(dirs: &AppDirs) -> HashMap<String, u32> {
    count_by_host(&lab_markers(dirs))
}

/// Runs still up on host `id` of labs no longer marked as running there (moved elsewhere,
/// uninstalled): only that host's credentials can take them down. As the runner records them
/// (any state but gone, on that host), and the launcher's own Terraform states not handed to the
/// runner yet that still hold resources there. A record that can't be read is an error.
pub(crate) fn leftovers_on(dirs: &AppDirs, id: &str) -> Result<usize> {
    let base = dirs.data()?;
    let elsewhere = |lab: &str| marked_host(&base.join("labs").join(lab)).as_deref() != Some(id);
    let runs = crate::runtime::isothrum::records(&crate::runtime::paths::runner_home(dirs)?)?
        .into_iter()
        .filter(|r| r.host_label.as_deref() == Some(id) && r.state != isothrum_runner::RunState::Absent)
        .filter_map(|r| r.name.as_str().strip_prefix(&crate::runtime::compose_project("")).map(str::to_owned))
        .filter(|lab| elsewhere(lab))
        .count();
    let states = crate::runtime::paths::deployed_ids(base)
        .iter()
        .filter(|lab| elsewhere(lab))
        .flat_map(|lab| crate::runtime::paths::deployment_states(base, lab))
        .filter(|(_, state)| crate::runtime::paths::recorded_host(state).as_deref() == Some(id) && crate::runtime::terraform::deployed(state))
        .count();
    Ok(runs + states)
}

/// The host id of each installed lab's marker (trimmed; empty when the marker is). A lab leaves
/// a `.cyberctf-host` marker in its directory while it runs on a host (written on start,
/// cleared on stop), so this is filesystem-only: no host calls, no credentials.
fn lab_markers(dirs: &AppDirs) -> Vec<String> {
    dirs.data().map(|d| markers_in(&d.join("labs"))).unwrap_or_default()
}

/// `lab_markers` for the labs folder `labs`.
fn markers_in(labs: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(labs) else { return Vec::new() };
    entries.flatten().filter_map(|e| marked_host(&e.path())).collect()
}

/// Lab counts per host id (markers with no id aren't counted).
fn count_by_host(markers: &[String]) -> HashMap<String, u32> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for id in markers.iter().filter(|id| !id.is_empty()) {
        *counts.entry(id.clone()).or_default() += 1;
    }
    counts
}

#[cfg(test)]
pub(crate) mod tests;
