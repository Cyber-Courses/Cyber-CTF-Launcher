//! The setup form's actions on the saved hosts: save one (with a password or token, or a Proxmox
//! node's SSH key, which makes the launcher's API token there), read a Proxmox server's placement
//! choices for the form, and remove a host. The rules on the saved hosts themselves are
//! `hosts`'.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use super::hosts::{keeps_placement, labs_on, no_labs_running, put_host, remove_host};
use super::input::{HostInput, valid_id};
use super::profile::{HostProfile, new_id};
use super::secrets::{delete_secret, get_secret, set_secret};
use super::store::{Store, load, load_host, update};
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;
use crate::runtime::proxmox::{placement, ssh_key};

/// Creates or updates a host from the form. The first host saved becomes the default. A host labs
/// are still deployed on keeps its place (see `hosts::keeps_placement`). A password or token save
/// keeps no SSH key sign-in (see [`save_with_ssh_key`]), but a re-save of an SSH-key setup whose
/// token is already made, with the same key, keeps it.
pub(crate) fn save(dirs: &AppDirs, mut input: HostInput) -> Result<HostProfile> {
    let saved = input.id.as_deref().and_then(|id| load(dirs).ok()?.hosts.into_iter().find(|h| h.id == id));
    if !saved.is_some_and(|h| input.same_ssh_key(&h)) {
        input.without_ssh_key();
    }
    put(dirs, input)
}

/// Saves a Proxmox host set up with the player's SSH key: signs in to the node with it, makes the
/// launcher's API token there and lets the launcher's key in (`proxmox::ssh_key`), then saves the
/// host as a token setup (the token's secret in the keychain) that remembers which key it used.
/// Nothing is saved when any of that fails.
pub(crate) async fn save_with_ssh_key(dirs: &AppDirs, mut input: HostInput) -> Result<HostProfile> {
    let key = input.ssh_key()?;
    // The address and the rest are checked before anything happens on the node.
    input.sign_in_as(format!("{}@pam", key.user), None);
    let checked = input.check()?;
    let (launcher, public) = crate::runtime::ssh::ensure_key(dirs).await?;
    let known_hosts = crate::runtime::ssh::node_known_hosts(&launcher);
    let made = ssh_key::bootstrap(&checked.host, &key, &known_hosts, &public).await.map_err(|f| Error::Invalid(f.to_string()))?;
    input.sign_in_as(made.token_id.clone(), Some(made.secret));
    let profile = put(dirs, input)?;
    let added = if made.key_added { "added to" } else { "already in" };
    let note = format!("Made the API token {} on the node with your key; the launcher's key was {added} {}'s authorized_keys.", made.token_id, key.user);
    MADE_WITH_KEY.lock().unwrap_or_else(PoisonError::into_inner).insert(profile.id.clone(), note);
    Ok(profile)
}

/// What the last SSH-key setup of each host made, by host id, for its Test connection.
static MADE_WITH_KEY: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// What the SSH-key setup of host `id` made in this run of the app, if it ran.
pub(super) fn made_with_key(id: &str) -> Option<String> {
    MADE_WITH_KEY.lock().unwrap_or_else(PoisonError::into_inner).get(id).cloned()
}

/// A Proxmox server's placement choices (nodes, storages, bridges), read from its API with the
/// form's sign-in: its password or token secret, else the saved host's.
pub(crate) async fn placement(dirs: &AppDirs, mut input: HostInput) -> Result<placement::Placement> {
    if input.provider != Provider::Proxmox {
        return Err(Error::Invalid("only Proxmox servers list their placement".into()));
    }
    input.without_ssh_key();
    let checked = input.check()?;
    let typed = input.password.take().filter(|p| !p.is_empty());
    let id = input.id.clone();
    let secret = match (typed, &id) {
        (Some(p), _) => p,
        (None, Some(id)) if load_host(dirs, id).is_ok() => get_secret(id)?,
        (None, _) => return Err(Error::Invalid("enter the password or token secret first".into())),
    };
    let profile = input.into_profile(id.unwrap_or_else(new_id), checked, None)?;
    let ip = tokio::net::lookup_host((profile.host.as_str(), profile.port)).await.ok().and_then(|mut a| a.next()).map(|a| a.ip());
    placement::read_host(&profile, &secret, ip).await.map_err(|e| Error::Invalid(format!("Couldn't read the server's nodes, storages and bridges: {e}")))
}

/// Removes host `id`, unless labs still run on it: this host holds the credentials and state
/// needed to stop and destroy them. Removing it mid-run would leave them unstoppable from the app,
/// and a cloud lab would keep billing with no way left to tear it down.
pub(crate) fn remove(dirs: &AppDirs, id: &str) -> Result<()> {
    no_labs_running(labs_on(dirs, id)?)?;
    // Its secret first: a keychain that refuses to let it go leaves the host as it was, rather
    // than a removed host whose password stays behind.
    delete_secret(id).map_err(|e| Error::Invalid(format!("couldn't remove this host's secret, so the host stays: {e}")))?;
    update(dirs, |store| {
        remove_host(store, id);
        Ok(())
    })
}

/// Saves a checked form: under its id, its secret in the keychain.
fn put(dirs: &AppDirs, mut input: HostInput) -> Result<HostProfile> {
    let checked = input.check()?;
    let deployed = match input.id.as_deref() {
        Some(id) => labs_on(dirs, id)?,
        None => 0,
    };
    update(dirs, |store| {
        let id = save_id(store, input.id.take())?;
        let gcp_project = kept_gcp_project(store, &id, input.provider, &checked.username);
        let password = input.password.take();
        let profile = input.into_profile(id.clone(), checked, gcp_project)?;
        if let Some(saved) = store.hosts.iter().find(|h| h.id == id) {
            keeps_placement(saved, &profile, deployed)?;
        }
        match new_password(password)? {
            Some(p) => set_secret(&id, &p)?,
            None if profile.keeps_secret() && get_secret(&id).is_err() => {
                return Err(Error::Invalid("enter the host's password".into()));
            }
            None => {}
        }
        put_host(store, profile.clone());
        Ok(profile)
    })
}

/// The id a save writes under: an existing host's (edit), or a new one.
fn save_id(store: &Store, id: Option<String>) -> Result<String> {
    match id {
        Some(id) if valid_id(&id) && store.hosts.iter().any(|h| h.id == id) => Ok(id),
        Some(_) => Err(Error::Invalid("unknown server host".into())),
        None => Ok(new_id()),
    }
}

/// GCP keeps its labs project across edits, unless the billing account changed.
fn kept_gcp_project(store: &Store, id: &str, provider: Provider, username: &str) -> Option<String> {
    store
        .hosts
        .iter()
        .find(|h| h.id == id && h.provider == Provider::Gcp && provider == Provider::Gcp && h.username == username)
        .and_then(|h| h.gcp_project.clone())
}

/// A password entered in the form, if one was (blank means "keep the stored one").
fn new_password(password: Option<String>) -> Result<Option<String>> {
    match password.filter(|p| !p.is_empty()) {
        Some(p) if p.len() <= 1024 && !p.contains('\0') => Ok(Some(p)),
        Some(_) => Err(Error::Invalid("invalid password".into())),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests;
