//! The rules of the saved hosts: which one is the default, what a host may change while labs are
//! on it, and how many labs are on each (as the runner records their runs there).

use std::collections::HashMap;

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

/// Drops host `id`, and only it; when it was the default, another server (never a cloud account)
/// takes over. A store with that id on more than one host is refused, as it is: a remove drops
/// one host or none.
pub(crate) fn remove_host(store: &mut Store, id: &str) -> Result<()> {
    if store.hosts.iter().filter(|h| h.id == id).count() > 1 {
        return Err(Error::Invalid(format!("more than one saved host has the id `{id}`, so none is removed")));
    }
    store.hosts.retain(|h| h.id != id);
    if store.default.as_deref() == Some(id) {
        // The default is a *server* to run VM labs on; a cloud account must never become it.
        store.default = store.hosts.iter().find(|h| !h.provider.is_cloud()).map(|h| h.id.clone());
    }
    Ok(())
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

/// How many labs still have something on host `id`, as the runner records their runs (any state
/// but gone, on that host): installed or not (moved elsewhere since, uninstalled), only that
/// host's credentials can take them down. A record that can't be read is an error.
pub(crate) async fn labs_on(dirs: &AppDirs, id: &str) -> Result<usize> {
    Ok(runs_on_hosts(dirs).await?.filter(|(host, _)| host == id).count())
}

/// How many installed labs run on each host, by host id, as the runner records their runs; none
/// counted when the records can't be read.
pub(crate) async fn running_by_host(dirs: &AppDirs) -> HashMap<String, u32> {
    let runs = match runs_on_hosts(dirs).await {
        Ok(runs) => runs,
        Err(e) => {
            log::warn!("couldn't count the labs on each host: {e}");
            return HashMap::new();
        }
    };
    let mut counts: HashMap<String, u32> = HashMap::new();
    for (host, _) in runs.filter(|(_, lab)| crate::runtime::lab_installed(dirs, lab)) {
        *counts.entry(host).or_default() += 1;
    }
    counts
}

/// Each run the runner records on a host (any state but gone): its host's id and its lab's id.
async fn runs_on_hosts(dirs: &AppDirs) -> Result<impl Iterator<Item = (String, String)>> {
    let runs = crate::runtime::isothrum::records(&crate::runtime::paths::runner_home(dirs)?).await?;
    Ok(runs.into_iter().filter(|r| r.state != isothrum_runner::RunState::Absent).filter_map(|r| {
        let lab = crate::runtime::lab_of_project(r.name.as_str())?.to_owned();
        Some((r.host_label?, lab))
    }))
}

#[cfg(test)]
pub(crate) mod tests;
