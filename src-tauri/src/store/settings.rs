//! The app's own settings: one JSON value per key.

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

/// The server VM labs run on when none is picked: a host id.
pub(crate) const DEFAULT_HOST: &str = "default_host";
/// The folder shared with the attack box (`shared_folder::SharedFolder`).
pub(crate) const SHARED_FOLDER: &str = "shared_folder";
/// This installation's id, the website agent's merge key (`account::agent::identity`).
pub(crate) const INSTALL_ID: &str = "install_id";

/// Setting `key`, when set.
///
/// # Errors
///
/// The store can't be read, or the value isn't a `T`.
pub(crate) fn get<T: serde::de::DeserializeOwned>(conn: &Connection, key: &str) -> Result<Option<T>> {
    let text: Option<String> = conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)).optional()?;
    Ok(text.map(|t| super::from_json(&t)).transpose()?)
}

/// Sets `key` to `value`, or clears it for `None`.
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn set<T: serde::Serialize>(conn: &Connection, key: &str, value: Option<&T>) -> Result<()> {
    match value {
        Some(v) => {
            conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value", (key, super::to_json(v)?))?
        }
        None => conn.execute("DELETE FROM settings WHERE key = ?1", [key])?,
    };
    Ok(())
}
