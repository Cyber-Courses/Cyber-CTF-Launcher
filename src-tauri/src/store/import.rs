//! The files the launcher kept its domain in before the store, imported once. Each source is
//! imported in one transaction with its `imported:<source>` mark, so it is never imported twice;
//! one that can't be read is logged, left unmarked and tried again at the next start (its file is
//! still there), and what it holds is added, never written over what the store has since. The
//! files stay where they are, never written again.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use super::Store;
use crate::error::{Error, Result, Wire};
use crate::machine::storage::owned::Owned;
use crate::machine::tools::uninstall::InstalledTool;
use crate::runtime::server::profile::HostProfile;

/// A file (or folder) kept before the store, and how its content goes in.
struct Source {
    /// Its name, in the mark and the log.
    name: &'static str,
    /// Its path under the data folder.
    path: &'static str,
    /// Adds what the file at the path holds to the store (in the import's transaction).
    add: fn(&Connection, &Path) -> Result<()>,
}

/// Every source, by its file under the data folder.
const SOURCES: &[Source] = &[
    Source { name: "servers", path: "server.json", add: add_hosts },
    Source { name: "storage", path: "storage.json", add: add_storage },
    Source { name: "shared_folder", path: "shared-folder.json", add: add_shared_folder },
    Source { name: "installed_tools", path: "installed-tools.json", add: add_tools },
    Source { name: "autostop_failures", path: "reap-errors", add: add_autostop_failures },
];

/// Imports every source of data folder `data` not imported yet (see the module).
pub(super) fn once(store: &Store, data: &Path) {
    for source in SOURCES {
        let mark = format!("imported:{}", source.name);
        let path = data.join(source.path);
        let imported = store.write(|tx| {
            if tx.query_row("SELECT 1 FROM meta WHERE key = ?1", [&mark], |_| Ok(())).optional()?.is_some() {
                return Ok(false);
            }
            if path.exists() {
                (source.add)(tx, &path)?;
            }
            tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2)", (&mark, super::now().to_string()))?;
            Ok(path.exists())
        });
        match imported {
            Ok(true) => log::info!("{} imported into the launcher's store", path.display()),
            Ok(false) => {}
            Err(e) => log::warn!("{} couldn't be imported into the launcher's store, tried again at the next start: {e}", path.display()),
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
    let default = super::hosts::default(conn)?.or(file.default);
    super::hosts::replace(conn, default.as_deref(), &hosts)
}

/// `storage.json`'s images and boxes.
fn add_storage(conn: &Connection, path: &Path) -> Result<()> {
    super::storage::add(conn, &read_json::<Owned>(path)?)
}

/// `shared-folder.json`'s setting, when the store has none.
fn add_shared_folder(conn: &Connection, path: &Path) -> Result<()> {
    if super::settings::get::<serde_json::Value>(conn, super::settings::SHARED_FOLDER)?.is_some() {
        return Ok(());
    }
    let setting: crate::shared_folder::SharedFolder = read_json(path)?;
    super::settings::set(conn, super::settings::SHARED_FOLDER, Some(&setting))
}

/// `installed-tools.json`'s records, each tool the store doesn't have.
fn add_tools(conn: &Connection, path: &Path) -> Result<()> {
    let mut tools = super::tools::all(conn)?;
    for tool in read_json::<Vec<InstalledTool>>(path)? {
        if !tools.iter().any(|t| t.dependency == tool.dependency) {
            tools.push(tool);
        }
    }
    super::tools::replace(conn, &tools)
}

/// The `reap-errors/` folder's records (`<lab id>`: `{message, error}`); one that isn't a record
/// is skipped (the next sweep says the failure again).
fn add_autostop_failures(conn: &Connection, dir: &Path) -> Result<()> {
    /// A record's shape.
    #[derive(serde::Deserialize)]
    struct Record {
        message: String,
        error: Wire,
    }
    let known = super::autostop::failed(conn)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(id) = entry.file_name().into_string() else { continue };
        if !crate::runtime::paths::is_lab_id(&id) || known.contains(&id) {
            continue;
        }
        match read_json::<Record>(&entry.path()) {
            Ok(r) => super::autostop::record(conn, &id, &r.message, &r.error)?,
            Err(e) => log::warn!("an auto-stop failure record that can't be read is left out: {e}"),
        }
    }
    Ok(())
}
