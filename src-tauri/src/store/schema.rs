//! The store's schema, by numbered migrations: SQLite's `user_version` is the number of the last
//! one applied, each runs once, in order, in its own transaction. A migration is never edited once
//! released: a change of the schema is a new one at the end.

use std::path::{Path, PathBuf};

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
#[cfg(test)]
pub(crate) fn current() -> u32 {
    u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX)
}

/// Applies the migrations `conn`'s store hasn't had yet (see [`migrate_with`]).
pub(super) fn migrate(conn: &mut Connection) -> Result<()> {
    migrate_with(conn, MIGRATIONS)
}

/// How many backups taken before a migration are kept: the last ones.
const BACKUPS_KEPT: usize = 2;

/// Applies `migrations` the store of `conn` hasn't had yet, each in its own transaction that reads
/// the version again first: another process opening the same store at once applies each one once.
/// Before the first one, the store is backed up beside its file, as it is at its schema version
/// (`<file>.v<version>.bak`, a consistent copy even in WAL mode; the last [`BACKUPS_KEPT`] kept):
/// a migration that fails, or a build that misreads what it wrote, leaves that copy to go back to.
/// A store from a newer build (a version past this one's) is refused: this build would misread
/// what it doesn't know.
fn migrate_with(conn: &mut Connection, migrations: &[&str]) -> Result<()> {
    let known = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
    let mut backed_up = false;
    loop {
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > known {
            return Err(Error::Invalid(format!("the launcher's store is from a newer Cyber CTF (schema {version}, this one knows {known}): update the app")));
        }
        if version == known {
            return Ok(());
        }
        if !backed_up {
            back_up(conn, version)?;
            backed_up = true;
        }
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Read again under the write lock: another process may have migrated meanwhile.
        let version: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let Some(sql) = migrations.get(version as usize) else { return Ok(()) };
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version + 1)?;
        tx.commit()?;
    }
}

/// The backup of the store at `file` taken at schema `version`.
pub(super) fn backup_path(file: &Path, version: u32) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".v{version}.bak"));
    file.with_file_name(name)
}

/// Copies `conn`'s store beside its file as it is at schema `version` (`VACUUM INTO`: a copy that
/// holds what the WAL holds too, never a raw copy of a file being written), then drops the oldest
/// backups past [`BACKUPS_KEPT`]. An in-memory store has nothing to back up.
fn back_up(conn: &Connection, version: u32) -> Result<()> {
    let Some(file) = conn.path().filter(|p| !p.is_empty()).map(PathBuf::from) else { return Ok(()) };
    let target = backup_path(&file, version);
    crate::platform::fs::remove_if_present(&target)?;
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    let dir = file.parent().unwrap_or(Path::new("."));
    let prefix = format!("{}.v", file.file_name().unwrap_or_default().to_string_lossy());
    let mut kept: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let v = name.strip_prefix(&prefix)?.strip_suffix(".bak")?.parse().ok()?;
            Some((v, e.path()))
        })
        .collect();
    kept.sort();
    let drop = kept.len().saturating_sub(BACKUPS_KEPT);
    for (_, old) in kept.into_iter().take(drop) {
        crate::platform::fs::remove_if_present(&old)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{backup_path, migrate_with};
    use rusqlite::Connection;

    fn version(conn: &Connection) -> u32 {
        conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_failed_migration_leaves_the_backup_taken_before_it_usable() {
        let dir = crate::testing::temp_dir("store-backup");
        let file = dir.join("launcher.sqlite3");
        let mut conn = Connection::open(&file).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        let one = ["CREATE TABLE kept (v TEXT) STRICT; INSERT INTO kept VALUES ('saved');"];
        migrate_with(&mut conn, &one).unwrap();
        // A migration that fails part way: rolled back, the backup at version 1 there and whole.
        let broken = [one[0], "CREATE TABLE next (v TEXT); INSERT INTO nowhere VALUES (1);"];
        assert!(migrate_with(&mut conn, &broken).is_err());
        assert_eq!(version(&conn), 1, "the failed migration changed nothing");
        let backup = Connection::open(backup_path(&file, 1)).unwrap();
        assert_eq!(version(&backup), 1);
        let saved: String = backup.query_row("SELECT v FROM kept", [], |r| r.get(0)).unwrap();
        assert_eq!(saved, "saved");
    }

    #[test]
    fn only_the_last_two_backups_are_kept_and_none_without_a_migration() {
        let dir = crate::testing::temp_dir("store-backups");
        let file = dir.join("launcher.sqlite3");
        let mut conn = Connection::open(&file).unwrap();
        let all = ["CREATE TABLE a (v INT);", "CREATE TABLE b (v INT);", "CREATE TABLE c (v INT);"];
        for n in 1..=3 {
            migrate_with(&mut conn, &all[..n]).unwrap();
        }
        let left: Vec<bool> = (0..3).map(|v| backup_path(&file, v).is_file()).collect();
        assert_eq!(left, [false, true, true], "v0 dropped, v1 and v2 kept");
        std::fs::remove_file(backup_path(&file, 2)).unwrap();
        migrate_with(&mut conn, &all).unwrap();
        assert!(!backup_path(&file, 3).is_file() && !backup_path(&file, 2).is_file(), "nothing to migrate: no backup");
    }
}
