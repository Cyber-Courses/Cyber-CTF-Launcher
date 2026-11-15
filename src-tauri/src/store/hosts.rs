//! The saved servers and cloud accounts: each one's non-secret settings, in the order they were
//! saved, and the default server. Their secrets are the keychain's.

use rusqlite::Connection;

use crate::error::Result;
use crate::runtime::server::profile::HostProfile;

/// Every saved host, in the order they were saved.
///
/// # Errors
///
/// The store can't be read, or a row isn't a host.
pub(crate) fn all(conn: &Connection) -> Result<Vec<HostProfile>> {
    let mut stmt = conn.prepare("SELECT profile FROM hosts ORDER BY position")?;
    let rows = stmt.query_map([], |row| super::from_json(&row.get::<_, String>(0)?))?;
    Ok(rows.collect::<rusqlite::Result<Vec<HostProfile>>>()?)
}

/// The default server's id, when one is set.
///
/// # Errors
///
/// The store can't be read.
pub(crate) fn default(conn: &Connection) -> Result<Option<String>> {
    super::settings::get(conn, super::settings::DEFAULT_HOST)
}

/// Saves `hosts` (in this order) and `default` as the whole set: a host not in it is removed.
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn replace(conn: &Connection, default: Option<&str>, hosts: &[HostProfile]) -> Result<()> {
    conn.execute("DELETE FROM hosts", [])?;
    let mut insert = conn.prepare("INSERT INTO hosts (id, position, provider, profile) VALUES (?1, ?2, ?3, ?4)")?;
    for (position, host) in hosts.iter().enumerate() {
        insert.execute((&host.id, i64::try_from(position).unwrap_or(i64::MAX), host.provider.id(), super::to_json(host)?))?;
    }
    super::settings::set(conn, super::settings::DEFAULT_HOST, default.as_ref())
}
