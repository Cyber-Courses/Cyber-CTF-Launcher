//! Where server and cloud profiles are kept: the launcher's store (`crate::store::hosts`; their
//! non-secret settings, secrets are in `secrets`).

use serde::{Deserialize, Serialize};

use super::profile::HostProfile;
use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// The saved servers and cloud accounts.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Store {
    /// The server VM labs run on when none is picked (never a cloud account).
    pub default: Option<String>,
    /// Every saved server and cloud account.
    pub hosts: Vec<HostProfile>,
}

/// The saved hosts in `conn`'s store.
fn read(conn: &rusqlite::Connection) -> Result<Store> {
    Ok(Store { default: crate::store::hosts::default(conn)?, hosts: crate::store::hosts::all(conn)? })
}

/// The saved hosts; none when nothing was saved yet.
pub(crate) fn load(dirs: &AppDirs) -> Result<Store> {
    crate::store::open(dirs)?.read(read)
}

/// Reads the saved hosts, applies `change` and saves them, in one transaction: two windows (or a
/// launch and a form) never write back a stale copy over each other's change. Nothing is saved
/// when `change` fails.
pub(crate) fn update<T>(dirs: &AppDirs, change: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    crate::store::open(dirs)?.write(|tx| {
        let mut store = read(tx)?;
        let out = change(&mut store)?;
        crate::store::hosts::replace(tx, store.default.as_deref(), &store.hosts)?;
        Ok(out)
    })
}

/// Saved host `id`; an error when there is none.
pub(crate) fn find(store: &Store, id: &str) -> Result<HostProfile> {
    store.hosts.iter().find(|h| h.id == id).cloned().ok_or_else(|| Error::Invalid(format!("server host `{id}` not found")))
}

/// A saved host by id, if the store has it. A store that can't be read is an error, never "no
/// such host": a lab on a server would read as one here, a cloud launch as a free one.
pub(super) fn saved_host(dirs: &AppDirs, id: &str) -> Result<Option<HostProfile>> {
    Ok(load(dirs)?.hosts.into_iter().find(|h| h.id == id))
}

/// A saved host by id, with the reason when it can't be had.
pub(crate) fn load_host(dirs: &AppDirs, id: &str) -> Result<HostProfile> {
    find(&load(dirs)?, id)
}

/// Test-only: makes saved host rows unreadable, as a damaged store would be.
#[cfg(test)]
pub(crate) fn damage(dirs: &AppDirs) {
    crate::store::open(dirs)
        .unwrap()
        .write(|tx| {
            tx.execute("UPDATE hosts SET profile = '{ not json'", [])?;
            Ok(())
        })
        .unwrap();
}

/// Test-only: forgets every saved host and the default, damaged or not.
#[cfg(test)]
pub(crate) fn clear(dirs: &AppDirs) {
    crate::store::open(dirs).unwrap().write(|tx| crate::store::hosts::replace(tx, None, &[])).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::providers::Provider;
    use crate::runtime::server::profile::tests::profile;

    #[test]
    fn a_missing_store_is_empty() {
        let t = crate::testing::test_app::TestApp::new();
        let store = load(&t.dirs()).unwrap();
        assert!(store.default.is_none() && store.hosts.is_empty());
    }

    #[test]
    fn the_store_round_trips() {
        let t = crate::testing::test_app::TestApp::new();
        let host = HostProfile { id: "ab12".into(), ..profile(Provider::Proxmox) };
        let other = HostProfile { id: "cd34".into(), ..profile(Provider::Aws) };
        update(&t.dirs(), |s| {
            *s = Store { default: Some("ab12".into()), hosts: vec![host.clone(), other.clone()] };
            Ok(())
        })
        .unwrap();
        let back = load(&t.dirs()).unwrap();
        assert_eq!(back.default.as_deref(), Some("ab12"));
        assert_eq!(back.hosts, vec![host.clone(), other], "in the order they were saved");
        assert_eq!(find(&back, "ab12").unwrap(), host);
        assert_eq!(find(&back, "zz").unwrap_err().to_string(), "server host `zz` not found");
    }

    #[test]
    fn an_unreadable_store_is_an_error_never_no_such_host() {
        // Regression: `saved_host` read a broken server list as "no such host": a lab on a server
        // then started here, and a website launch onto a cloud account skipped its cost prompt.
        let t = crate::testing::test_app::TestApp::new();
        let id = t.host("pve", "proxmox", serde_json::json!({}));
        assert_eq!(saved_host(&t.dirs(), &id).unwrap().map(|h| h.provider), Some(Provider::Proxmox));
        damage(&t.dirs());
        assert!(saved_host(&t.dirs(), &id).is_err());
        assert!(super::super::host_provider(&t.dirs(), &id).is_err());
        assert!(super::super::host_name(&t.dirs(), &id).is_err());
    }

    #[test]
    fn concurrent_changes_all_land_and_a_failed_one_saves_nothing() {
        // Each change reads in its own write transaction: none writes back a stale copy over another's.
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let dirs = dirs.clone();
                std::thread::spawn(move || {
                    update(&dirs, |s| {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        s.hosts.push(HostProfile { id: format!("h{i}"), ..profile(Provider::Proxmox) });
                        Ok(())
                    })
                    .unwrap();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(load(&dirs).unwrap().hosts.len(), 8);
        assert!(
            update(&dirs, |s| -> Result<()> {
                s.hosts.clear();
                Err(Error::Invalid("no".into()))
            })
            .is_err()
        );
        assert_eq!(load(&dirs).unwrap().hosts.len(), 8);
    }
}
