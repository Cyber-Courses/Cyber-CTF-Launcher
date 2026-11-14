//! The plain (non-secret) values a cloud run was started with: its module's region, firewall
//! CIDR, profile and placement. Terraform needs the same values for every later operation (a
//! destroy given others can fail, or act on another region), and the runner keeps none of them,
//! so the launcher keeps what it gave. Secrets are never here: they come from the keychain each
//! time.

use rusqlite::Connection;

use crate::error::Result;

/// Run `run`'s values, by name.
///
/// # Errors
///
/// The store can't be read.
pub(crate) fn of(conn: &Connection, run: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT name, value FROM run_values WHERE run = ?1 ORDER BY name")?;
    Ok(stmt.query_map([run], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
}

/// Keeps `values` as run `run`'s, in place of any it had.
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn replace(conn: &Connection, run: &str, values: &[(String, String)]) -> Result<()> {
    forget(conn, run)?;
    let mut insert = conn.prepare("INSERT INTO run_values (run, name, value) VALUES (?1, ?2, ?3)")?;
    for (name, value) in values {
        insert.execute((run, name, value))?;
    }
    Ok(())
}

/// Forgets run `run`'s values, once it is gone.
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn forget(conn: &Connection, run: &str) -> Result<()> {
    conn.execute("DELETE FROM run_values WHERE run = ?1", [run])?;
    Ok(())
}
