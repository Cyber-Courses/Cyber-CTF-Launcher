//! The tools the guided setup installed, so Cleanup removes only those.

use rusqlite::Connection;

use crate::error::Result;
use crate::machine::tools::install::Dependency;
use isothrum_machine::install::record::InstalledTool;

/// `dependency`'s key, as the webview names it.
fn key(dependency: Dependency) -> Result<String> {
    Ok(super::to_json(&dependency)?.trim_matches('"').to_owned())
}

/// Every recorded tool, in the order they were recorded.
///
/// # Errors
///
/// The store can't be read, or a row names no tool this build knows.
pub(crate) fn all(conn: &Connection) -> Result<Vec<InstalledTool>> {
    let mut stmt = conn.prepare("SELECT dependency, at, pending FROM installed_tools ORDER BY at, rowid")?;
    let rows = stmt.query_map([], |row| {
        let dependency: Dependency = super::from_json(&format!("\"{}\"", row.get::<_, String>(0)?))?;
        Ok(InstalledTool { dependency, at: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0), pending: row.get(2)? })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Saves `tools` as the whole record.
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn replace(conn: &Connection, tools: &[InstalledTool]) -> Result<()> {
    conn.execute("DELETE FROM installed_tools", [])?;
    let mut insert = conn.prepare("INSERT OR REPLACE INTO installed_tools (dependency, at, pending) VALUES (?1, ?2, ?3)")?;
    for t in tools {
        insert.execute((key(t.dependency)?, i64::try_from(t.at).unwrap_or(i64::MAX), t.pending))?;
    }
    Ok(())
}
