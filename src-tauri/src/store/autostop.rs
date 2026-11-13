//! Why the auto-stop couldn't destroy a cloud lab the last time it tried: kept for the lab's page
//! while the lab is still billing, and so a failure is notified once, not at every sweep.

use rusqlite::{Connection, OptionalExtension};

use crate::error::{Result, Wire};

/// The labs the last sweep couldn't destroy.
///
/// # Errors
///
/// The store can't be read.
pub(crate) fn failed(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT lab_id FROM autostop_failures ORDER BY lab_id")?;
    Ok(stmt.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?)
}

/// Lab `id`'s last failure, as the webview reads an error.
///
/// # Errors
///
/// The store can't be read, or the row isn't an error.
pub(crate) fn last(conn: &Connection, id: &str) -> Result<Option<Wire>> {
    let text: Option<String> = conn.query_row("SELECT error FROM autostop_failures WHERE lab_id = ?1", [id], |row| row.get(0)).optional()?;
    Ok(text.map(|t| super::from_json(&t)).transpose()?)
}

/// Replaces the sweep's failures with `failures` (lab id, message, error): a lab destroyed this
/// time has its row go.
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn replace(conn: &Connection, failures: &[(String, String, Wire)]) -> Result<()> {
    conn.execute("DELETE FROM autostop_failures", [])?;
    let mut insert = conn.prepare("INSERT OR REPLACE INTO autostop_failures (lab_id, message, error, at) VALUES (?1, ?2, ?3, ?4)")?;
    for (id, message, error) in failures {
        insert.execute((id, message, super::to_json(error)?, super::now()))?;
    }
    Ok(())
}
