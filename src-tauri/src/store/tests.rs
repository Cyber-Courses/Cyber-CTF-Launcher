//! The store: a fresh one, its transactions and its migrations.

use std::path::{Path, PathBuf};

use super::{autostop, hosts, settings};
use crate::error::{Code, Error, Wire};

/// A data folder of this test's own, closed and removed when it drops.
struct Data(PathBuf);

impl Data {
    fn new(tag: &str) -> Data {
        let dir = std::env::temp_dir().join(format!("cyberctf-store-{tag}-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        Data(dir)
    }

    fn store(&self) -> super::Store {
        super::open_in(&self.0).unwrap()
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        super::forget(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_fresh_data_folder_opens_empty_in_wal_mode_at_the_current_schema() {
    let data = Data::new("fresh");
    let store = data.store();
    let (mode, version): (String, u32) =
        store.read(|c| Ok((c.pragma_query_value(None, "journal_mode", |r| r.get(0))?, c.pragma_query_value(None, "user_version", |r| r.get(0))?))).unwrap();
    assert_eq!((mode.as_str(), version), ("wal", super::schema::current()));
    assert_eq!(store.read(hosts::all).unwrap(), []);
}

#[test]
fn a_store_from_a_newer_app_is_refused_never_misread() {
    let data = Data::new("newer");
    data.store().write(|tx| Ok(tx.pragma_update(None, "user_version", super::schema::current() + 1)?)).unwrap();
    super::forget(&data.0);
    let err = super::open_in(&data.0).unwrap_err().to_string();
    assert!(err.contains("newer Cyber CTF"), "{err}");
}

#[test]
fn a_failed_change_leaves_the_store_as_it_was() {
    let data = Data::new("rollback");
    let store = data.store();
    let w = Wire { code: Code::Other, message: "x".into() };
    store.write(|tx| autostop::replace(tx, &[("a".into(), "m".into(), w.clone())])).unwrap();
    let failed = store.write(|tx| -> crate::error::Result<()> {
        autostop::replace(tx, &[("b".into(), "m".into(), w.clone())])?;
        Err(Error::Invalid("no".into()))
    });
    assert!(failed.is_err());
    assert_eq!(store.read(autostop::failed).unwrap(), ["a"], "rolled back whole");
}

#[test]
fn read_modify_writes_from_threads_and_processes_each_keep_the_others_change() {
    // Two connections to one file stand for the app and a helper process: each change is taken
    // for writing from its start, so none saves a stale copy over another's.
    let data = Data::new("race");
    let app = data.store();
    let worker = super::Store::open_file(&data.0).unwrap();
    std::thread::scope(|s| {
        for (n, store) in [&app, &worker, &app, &worker].into_iter().enumerate() {
            s.spawn(move || {
                for i in 0..10 {
                    store
                        .write(|tx| {
                            let mut seen: Vec<String> = settings::get(tx, "race")?.unwrap_or_default();
                            seen.push(format!("r{n}-{i}"));
                            settings::set(tx, "race", Some(&seen))
                        })
                        .unwrap();
                }
            });
        }
    });
    let seen: Vec<String> = app.read(|c| settings::get(c, "race")).unwrap().unwrap();
    assert_eq!(seen.len(), 40);
}

#[test]
fn the_same_folder_is_one_store_per_process() {
    let data = Data::new("one");
    let (a, b) = (data.store(), data.store());
    assert!(std::sync::Arc::ptr_eq(&a.conn, &b.conn));
    let _: &Path = &data.0;
}
