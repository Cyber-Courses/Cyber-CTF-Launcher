//! Where server and cloud profiles are kept: `server.json` in app data (non-secret settings;
//! secrets are in `secrets`).

use std::path::{Path, PathBuf};

use crate::AppHandle;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::profile::HostProfile;
use crate::error::{Error, Result};

const STORE_FILE: &str = "server.json";

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Store {
    /// The server VM labs run on when none is picked (never a cloud account).
    pub default: Option<String>,
    pub hosts: Vec<HostProfile>,
}

fn store_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(STORE_FILE))
}

pub(super) fn load(app: &AppHandle) -> Result<Store> {
    load_from(&store_path(app)?)
}

/// The store at `path`; an empty one when the file doesn't exist yet.
fn load_from(path: &Path) -> Result<Store> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| Error::Invalid(format!("server.json: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(e.into()),
    }
}

/// Serialises every change to `server.json`: each one reloads the file, changes it and saves it
/// under this lock, so two windows (or a launch and a form) never write back a stale copy over
/// each other's change.
static WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Reloads the store, applies `change` and saves it, all under the store lock. Nothing is saved
/// when `change` fails.
pub(super) fn update<T>(app: &AppHandle, change: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    update_at(&store_path(app)?, change)
}

fn update_at<T>(path: &Path, change: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    let _guard = WRITES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut store = load_from(path)?;
    let out = change(&mut store)?;
    save_to(path, &store)?;
    Ok(out)
}

/// Writes the store to `path` (pretty JSON), atomically: a crash mid-write keeps the old file.
fn save_to(path: &Path, store: &Store) -> Result<()> {
    let raw = serde_json::to_string_pretty(store).map_err(|e| Error::Invalid(e.to_string()))?;
    crate::atomic_file::write(path, raw)?;
    Ok(())
}

pub(super) fn find(store: &Store, id: &str) -> Result<HostProfile> {
    store.hosts.iter().find(|h| h.id == id).cloned().ok_or_else(|| Error::Invalid(format!("server host `{id}` not found")))
}

/// A saved host by id, if the store reads and has it.
pub(super) fn saved_host(app: &AppHandle, id: &str) -> Option<HostProfile> {
    load(app).ok()?.hosts.into_iter().find(|h| h.id == id)
}

/// A saved host by id, with the reason when it can't be had.
pub(super) fn load_host(app: &AppHandle, id: &str) -> Result<HostProfile> {
    find(&load(app)?, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::providers::Provider;
    use crate::runtime::server::profile::tests::profile;

    /// A fresh folder for one test, and the store file's path in it.
    fn temp_store(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-store-{tag}-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(STORE_FILE)
    }

    #[test]
    fn a_missing_store_is_empty() {
        let path = temp_store("missing");
        let store = load_from(&path).unwrap();
        assert!(store.default.is_none() && store.hosts.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_store_round_trips_through_its_file() {
        let path = temp_store("roundtrip");
        let host = HostProfile { id: "ab12".into(), ..profile(Provider::Proxmox) };
        save_to(&path, &Store { default: Some("ab12".into()), hosts: vec![host.clone()] }).unwrap();
        let back = load_from(&path).unwrap();
        assert_eq!(back.default.as_deref(), Some("ab12"));
        assert_eq!(back.hosts, vec![host.clone()]);
        assert_eq!(find(&back, "ab12").unwrap(), host);
        assert_eq!(find(&back, "zz").unwrap_err().to_string(), "server host `zz` not found");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_changes_all_land() {
        // Each change reloads under the lock: none writes back a stale copy over another's.
        let path = temp_store("concurrent");
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    update_at(&path, |s| {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        s.hosts.push(HostProfile { id: format!("h{i}"), ..profile(Provider::Proxmox) });
                        Ok(())
                    })
                    .unwrap()
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(load_from(&path).unwrap().hosts.len(), 8);
        // A failed change saves nothing.
        assert!(
            update_at(&path, |s| -> Result<()> {
                s.hosts.clear();
                Err(Error::Invalid("no".into()))
            })
            .is_err()
        );
        assert_eq!(load_from(&path).unwrap().hosts.len(), 8);
        // Written whole: only the store itself in its folder, no temporary file left.
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_corrupt_store_names_the_file() {
        let path = temp_store("corrupt");
        std::fs::write(&path, "{not json").unwrap();
        let err = load_from(&path).err().unwrap().to_string();
        assert!(err.starts_with("server.json: "), "{err}");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_unreadable_store_is_an_io_error() {
        // A folder where the file should be: reading it fails, and not as "not found".
        let path = temp_store("dir");
        std::fs::create_dir_all(&path).unwrap();
        assert!(matches!(load_from(&path), Err(Error::Io(_))));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
