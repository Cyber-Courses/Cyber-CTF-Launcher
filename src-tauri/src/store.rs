//! The launcher's own state, one SQLite file in its data folder (`launcher.sqlite3`): the saved
//! servers and cloud accounts (their secrets stay in the keychain), the tools the setup installed,
//! what the app downloaded for itself, the shared folder setting and the auto-stop's failures. Runs
//! are not here: the runner owns everything about a run, the values its start was given included.
//!
//! WAL mode, so the app and its helper processes read while one writes; every change is one
//! transaction ([`Store::write`]), taken for writing from its start, so a read-modify-write never
//! saves over a change made beside it. The schema moves forward by numbered migrations
//! ([`schema`]), the store backed up beside its file before each run of them (the last two
//! kept). A fresh data folder starts with an empty store: the JSON files earlier versions
//! kept are not read.

pub(crate) mod schema;
#[cfg(test)]
mod tests;

pub(crate) mod autostop;
pub(crate) mod hosts;
pub(crate) mod settings;
pub(crate) mod storage;
pub(crate) mod tools;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// The store's file, in the app data folder.
pub(crate) const FILE: &str = "launcher.sqlite3";

/// How long a write waits for another process's write to end before it fails.
const BUSY_WAIT: Duration = Duration::from_secs(15);

/// The launcher's store in one data folder. Cheap to clone: one connection per data folder and
/// process, shared.
#[derive(Clone)]
pub(crate) struct Store {
    /// The open connection, one at a time.
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

/// The open stores of this process, by data folder.
fn opened() -> &'static Mutex<HashMap<PathBuf, Store>> {
    static OPENED: OnceLock<Mutex<HashMap<PathBuf, Store>>> = OnceLock::new();
    OPENED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The store of the app's data folder (see [`open_in`]).
///
/// # Errors
///
/// No data folder, or the store can't be opened or migrated.
pub(crate) fn open(dirs: &AppDirs) -> Result<Store> {
    open_in(dirs.data()?)
}

/// The store in data folder `data`: opened and migrated; the same store for the rest of the
/// process's life.
///
/// # Errors
///
/// The folder can't be made, or the store can't be opened or migrated.
pub(crate) fn open_in(data: &Path) -> Result<Store> {
    let mut stores = opened().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(store) = stores.get(data) {
        return Ok(store.clone());
    }
    let store = Store::open_file(data)?;
    stores.insert(data.to_path_buf(), store.clone());
    Ok(store)
}

/// Test-only: closes data folder `data`'s store (a test's app removes its folder).
#[cfg(test)]
pub(crate) fn forget(data: &Path) {
    opened().lock().unwrap_or_else(PoisonError::into_inner).remove(data);
}

impl Store {
    /// Opens (creating) the store's file in `data`, in WAL mode, migrated to the current schema.
    fn open_file(data: &Path) -> Result<Store> {
        std::fs::create_dir_all(data)?;
        let mut conn = Connection::open(data.join(FILE))?;
        conn.busy_timeout(BUSY_WAIT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate(&mut conn)?;
        Ok(Store { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Reads with `query`, outside any write.
    ///
    /// # Errors
    ///
    /// What `query` returns.
    pub(crate) fn read<T>(&self, query: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        query(&conn)
    }

    /// Runs `change` in one transaction, taken for writing from its start (so what it reads is
    /// what it writes over): committed when it returns `Ok`, rolled back, nothing changed, when it
    /// fails.
    ///
    /// # Errors
    ///
    /// What `change` returns, or the transaction can't begin or commit.
    pub(crate) fn write<T>(&self, change: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = change(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

impl From<rusqlite::Error> for Error {
    /// A store failure, said as one.
    fn from(e: rusqlite::Error) -> Error {
        Error::Invalid(format!("the launcher's store: {e}"))
    }
}

/// `value` as the JSON text a column keeps.
pub(crate) fn to_json(value: &impl serde::Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Invalid(format!("the launcher's store: {e}")))
}

/// A column's JSON text read back as `T`; a value that isn't one is the store's error, named.
pub(crate) fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

/// Seconds since the Unix epoch, as the store's times are.
pub(crate) fn now() -> i64 {
    i64::try_from(crate::platform::clock::unix_secs()).unwrap_or(i64::MAX)
}
