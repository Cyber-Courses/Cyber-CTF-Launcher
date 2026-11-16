//! The files the launcher kept its own state in before the store (1.9 and earlier), taken in once:
//! the saved servers (`server.json`), the shared folder (`shared-folder.json`) and the install id
//! (`install-id`). Each file is read into the store in one transaction that adds what the store
//! hasn't (never over what it holds), then removed: its removal is the mark that it was taken, so
//! a start that couldn't remove it takes it again, which adds nothing. A file that can't be read
//! is logged and left where it is for the next start: nothing a player saved is lost. The
//! auto-stop's old failure records (`reap-errors/`) are only removed: the next sweep says a
//! failure again.
//!
//! Not here: the download ledger (`storage.json`) and the install record
//! (`installed-tools.json`), which move out of the launcher (to Isothrum's runner and its
//! machine crate).

use std::path::Path;

use rusqlite::Connection;

use super::Store;
use crate::error::{Error, Result};
use crate::runtime::server::profile::HostProfile;

/// A file kept before the store, and how its content goes in.
struct Source {
    /// Its name under the data folder.
    file: &'static str,
    /// Adds what the file at the path holds to the store, in the import's transaction.
    add: fn(&Connection, &Path) -> Result<()>,
    /// The files beside it that go with it (its lock).
    with: &'static [&'static str],
}

/// Every file taken in, by its name under the data folder.
const SOURCES: &[Source] = &[
    Source { file: "server.json", add: add_hosts, with: &[] },
    Source { file: "shared-folder.json", add: add_shared_folder, with: &[] },
    Source { file: "install-id", add: add_install_id, with: &["install-id.lock"] },
];

/// The folders earlier versions kept that hold nothing to take in.
const DROPPED: &[&str] = &["reap-errors"];

/// Takes in every file of data folder `data` still there (see the module).
pub(super) fn take_in(store: &Store, data: &Path) {
    for source in SOURCES {
        let path = data.join(source.file);
        if !path.exists() {
            continue;
        }
        if let Err(e) = store.write(|tx| (source.add)(tx, &path)) {
            log::warn!("{} couldn't be taken into the launcher's store, tried again at the next start: {e}", path.display());
            continue;
        }
        log::info!("{} was taken into the launcher's store", path.display());
        for file in std::iter::once(source.file).chain(source.with.iter().copied()) {
            if let Err(e) = crate::platform::fs::remove_if_present(&data.join(file)) {
                log::warn!("{file} was taken into the launcher's store but couldn't be removed (taken again, adding nothing, at the next start): {e}");
            }
        }
    }
    for dir in DROPPED {
        if let Err(e) = crate::platform::fs::remove_dir_if_present(&data.join(dir)) {
            log::warn!("{dir}, which this version no longer reads, couldn't be removed: {e}");
        }
    }
}

/// The JSON file at `path` read as a `T`.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))
}

/// `server.json`'s hosts (each one the store doesn't have, after its own) and its default (when
/// the store has none).
fn add_hosts(conn: &Connection, path: &Path) -> Result<()> {
    /// `server.json`'s shape.
    #[derive(serde::Deserialize)]
    struct File {
        default: Option<String>,
        #[serde(default)]
        hosts: Vec<HostProfile>,
    }
    let file: File = read_json(path)?;
    let mut hosts = super::hosts::all(conn)?;
    for host in file.hosts {
        if !hosts.iter().any(|h| h.id == host.id) {
            hosts.push(host);
        }
    }
    let default = super::hosts::default(conn)?.or(file.default).filter(|d| hosts.iter().any(|h| &h.id == d));
    super::hosts::replace(conn, default.as_deref(), &hosts)
}

/// `shared-folder.json`'s setting, when the store has none.
fn add_shared_folder(conn: &Connection, path: &Path) -> Result<()> {
    if super::settings::get::<serde_json::Value>(conn, super::settings::SHARED_FOLDER)?.is_some() {
        return Ok(());
    }
    let setting: crate::shared_folder::SharedFolder = read_json(path)?;
    super::settings::set(conn, super::settings::SHARED_FOLDER, Some(&setting))
}

/// `install-id`'s id, when the store has none: the website knows this machine by it.
fn add_install_id(conn: &Connection, path: &Path) -> Result<()> {
    let id = std::fs::read_to_string(path)?;
    let id = id.trim();
    if id.is_empty() || super::settings::get::<String>(conn, super::settings::INSTALL_ID)?.is_some() {
        return Ok(());
    }
    super::settings::set(conn, super::settings::INSTALL_ID, Some(&id))
}
