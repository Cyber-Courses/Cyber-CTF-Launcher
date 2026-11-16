//! The store: a fresh one, its transactions and its migrations, and the files of earlier
//! versions taken in.

use std::path::{Path, PathBuf};

use super::{autostop, hosts, settings, storage, tools};
use crate::error::{Code, Error, Wire};

/// A data folder of this test's own, closed and removed when it drops.
struct Data(PathBuf);

impl Data {
    fn new(tag: &str) -> Data {
        let dir = std::env::temp_dir().join(format!("cyberctf-store-{tag}-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        Data(dir)
    }

    fn write(&self, name: &str, body: &str) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
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

/// `server.json` as 1.9 wrote it: a Proxmox server (default) and an AWS account, fields of later
/// versions absent.
const SERVER_JSON_1_9: &str = r#"{
  "default": "3f2a9c1d0b8e7f65",
  "hosts": [
    { "id": "3f2a9c1d0b8e7f65", "name": "Garage", "provider": "proxmox", "host": "pve.lan", "port": 8006, "username": "root@pam!cyberctf-0a1b2c3d",
      "datastore": "local-lvm", "network": "vmbr1", "node": "pve", "insecureTls": true, "autoStopHours": null, "useCliCreds": false, "awsProfile": null },
    { "id": "aws1", "name": "AWS", "provider": "aws", "host": "eu-west-3", "port": 0, "username": "", "datastore": null, "network": null, "node": null,
      "insecureTls": false, "autoStopHours": 4, "useCliCreds": true, "awsProfile": "default" }
  ]
}"#;

#[test]
fn the_files_1_9_kept_its_state_in_are_taken_in_once_then_removed() {
    let data = Data::new("legacy");
    data.write("server.json", SERVER_JSON_1_9);
    data.write("shared-folder.json", r#"{ "enabled": true, "path": "/home/player/CyberCTF/shared" }"#);
    data.write("install-id", "0123456789abcdef0123456789abcdef\n");
    data.write("install-id.lock", "");
    data.write("reap-errors/cyberctf-web", r#"{ "message": "m", "error": { "code": "other", "message": "x" } }"#);
    data.write("installed-tools.json", r#"[ { "dependency": "vagrant", "at": 1700000000 } ]"#);
    let store = data.store();
    let hosts = store.read(hosts::all).unwrap();
    assert_eq!(hosts.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["3f2a9c1d0b8e7f65", "aws1"]);
    assert_eq!(hosts[0].name, "Garage");
    assert!(hosts[0].insecure_tls && hosts[0].tls_fingerprint.is_none(), "a later field at its default");
    assert_eq!(hosts[1].auto_stop_hours, Some(4));
    assert_eq!(store.read(hosts::default).unwrap().as_deref(), Some("3f2a9c1d0b8e7f65"));
    let shared: crate::shared_folder::SharedFolder = store.read(|c| settings::get(c, settings::SHARED_FOLDER)).unwrap().unwrap();
    assert_eq!((shared.enabled, shared.path.as_deref()), (true, Some("/home/player/CyberCTF/shared")));
    let id: String = store.read(|c| settings::get(c, settings::INSTALL_ID)).unwrap().unwrap();
    assert_eq!(id, "0123456789abcdef0123456789abcdef", "the website knows this machine by it");
    for gone in ["server.json", "shared-folder.json", "install-id", "install-id.lock", "reap-errors"] {
        assert!(!data.0.join(gone).exists(), "{gone} removed once taken in");
    }
    assert!(data.0.join("installed-tools.json").exists(), "the install record is the machine crate's to move, left alone");
    assert_eq!(store.read(tools::all).unwrap().len(), 0);
}

#[test]
fn a_file_taken_in_again_adds_only_what_the_store_lacks_and_never_writes_over_it() {
    // A start that took the files in but couldn't remove them: the next start takes them again.
    let data = Data::new("legacy-again");
    let store = data.store();
    let mine = crate::runtime::server::profile::tests::profile(crate::runtime::providers::Provider::Proxmox);
    store
        .write(|tx| {
            hosts::replace(tx, Some(&mine.id), std::slice::from_ref(&mine))?;
            settings::set(tx, settings::INSTALL_ID, Some(&"kept-id"))?;
            settings::set(tx, settings::SHARED_FOLDER, Some(&crate::shared_folder::SharedFolder { enabled: false, path: None }))
        })
        .unwrap();
    data.write("server.json", SERVER_JSON_1_9);
    data.write("shared-folder.json", r#"{ "enabled": true, "path": "/elsewhere" }"#);
    data.write("install-id", "another-id");
    super::legacy::take_in(&store, &data.0);
    let ids: Vec<String> = store.read(hosts::all).unwrap().into_iter().map(|h| h.id).collect();
    assert_eq!(ids, [mine.id.clone(), "3f2a9c1d0b8e7f65".into(), "aws1".into()], "the store's first, then the file's it lacked");
    assert_eq!(store.read(hosts::default).unwrap(), Some(mine.id), "the store's default kept");
    let id: String = store.read(|c| settings::get(c, settings::INSTALL_ID)).unwrap().unwrap();
    assert_eq!(id, "kept-id");
    let shared: crate::shared_folder::SharedFolder = store.read(|c| settings::get(c, settings::SHARED_FOLDER)).unwrap().unwrap();
    assert!(!shared.enabled, "the store's setting kept");
}

#[test]
fn a_file_that_cant_be_read_is_left_for_the_next_start_and_the_others_are_taken() {
    let data = Data::new("legacy-broken");
    data.write("server.json", "{ cut short");
    data.write("install-id", "0123456789abcdef0123456789abcdef");
    crate::testing::logs::take_warnings();
    let store = data.store();
    let warned = crate::testing::logs::take_warnings();
    assert!(warned.iter().any(|w| w.contains("server.json") && w.contains("tried again at the next start")), "{warned:?}");
    assert_eq!(std::fs::read_to_string(data.0.join("server.json")).unwrap(), "{ cut short", "left as it was: nothing lost");
    assert_eq!(store.read(hosts::all).unwrap(), []);
    assert!(!data.0.join("install-id").exists(), "the readable one taken in");
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
                            let mut owned = storage::load(tx)?;
                            owned.images.insert(crate::machine::storage::owned::OwnedImage { reference: format!("r{n}-{i}"), id: "x".into() });
                            storage::replace(tx, &owned)
                        })
                        .unwrap();
                }
            });
        }
    });
    assert_eq!(app.read(storage::load).unwrap().images.len(), 40);
}

#[test]
fn the_same_folder_is_one_store_per_process() {
    let data = Data::new("one");
    let (a, b) = (data.store(), data.store());
    assert!(std::sync::Arc::ptr_eq(&a.conn, &b.conn));
    let _: &Path = &data.0;
}
