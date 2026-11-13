//! The store's schema, by numbered migrations: SQLite's `user_version` is the number of the last
//! one applied, each runs once, in order, in its own transaction. A migration is never edited once
//! released: a change of the schema is a new one at the end.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// The migrations, in order: the schema after the n-th is version n.
const MIGRATIONS: &[&str] = &[
    // 1: the launcher's domain.
    "
    -- The app's own settings, each a JSON value by its key (`settings`).
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT;
    -- The saved servers and cloud accounts, in the order they were saved: their non-secret
    -- settings (`HostProfile`, as JSON), the secrets in the keychain.
    CREATE TABLE hosts (
        id       TEXT PRIMARY KEY,
        position INTEGER NOT NULL,
        provider TEXT NOT NULL,
        profile  TEXT NOT NULL
    ) STRICT;
    -- The tools the guided setup installed, so Cleanup removes only those.
    CREATE TABLE installed_tools (
        dependency TEXT PRIMARY KEY,
        at         INTEGER NOT NULL,
        pending    INTEGER NOT NULL DEFAULT 0 CHECK (pending IN (0, 1))
    ) STRICT;
    -- What the app downloaded for its own operations: images by reference and id, boxes.
    CREATE TABLE storage_images (
        reference TEXT NOT NULL,
        image_id  TEXT NOT NULL,
        PRIMARY KEY (reference, image_id)
    ) STRICT;
    CREATE TABLE storage_boxes (
        name     TEXT NOT NULL,
        provider TEXT NOT NULL,
        version  TEXT NOT NULL,
        PRIMARY KEY (name, provider, version)
    ) STRICT;
    -- Why the last auto-stop sweep couldn't destroy a cloud lab (the error as the webview reads it).
    CREATE TABLE autostop_failures (
        lab_id  TEXT PRIMARY KEY,
        message TEXT NOT NULL,
        error   TEXT NOT NULL,
        at      INTEGER NOT NULL
    ) STRICT;
    ",
];

/// The schema version this build writes.
pub(crate) fn current() -> u32 {
    u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX)
}

/// Applies the migrations `conn`'s store hasn't had yet, each in its own transaction that reads
/// the version again first: another process opening the same store at once applies each one once.
/// A store from a newer build (a version past this one's) is refused: this build would misread what
/// it doesn't know.
pub(super) fn migrate(conn: &mut Connection) -> Result<()> {
    loop {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > current() {
            return Err(Error::Invalid(format!(
                "the launcher's store is from a newer Cyber CTF (schema {version}, this one knows {}): update the app",
                current()
            )));
        }
        let Some(sql) = MIGRATIONS.get(version as usize) else { return Ok(()) };
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version + 1)?;
        tx.commit()?;
    }
}
