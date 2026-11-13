//! The store: its import of a data folder as the launcher kept it before, its transactions and its
//! migrations.

use std::path::{Path, PathBuf};

use super::{autostop, hosts, settings, storage, tools};
use crate::error::{Code, Error, Wire};
use crate::machine::tools::install::Dependency;

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

    /// Closes the store, so the next open is a new start of the app.
    fn restart(&self) -> super::Store {
        super::forget(&self.0);
        self.store()
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        super::forget(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A data folder as a 1.9 launcher left it: two servers (one the default), a cloud account, what
/// it downloaded, the shared folder on, two tools installed (one waiting for its installer), and
/// one lab the auto-stop couldn't destroy.
fn fixture(data: &Data) {
    data.write(
        "server.json",
        r#"{
  "default": "pve1",
  "hosts": [
    { "id": "pve1", "name": "Garage Proxmox", "provider": "proxmox", "host": "10.1.2.3", "port": 8006, "username": "root@pam!cyberctf", "datastore": "local-lvm", "network": "vmbr0", "node": "pve", "insecureTls": true },
    { "id": "esx", "name": "ESXi", "provider": "vmware_esxi", "host": "esx.lan", "port": 22, "username": "root" },
    { "id": "aws1", "name": "My AWS", "provider": "aws", "host": "eu-west-3", "port": 443, "username": "AKIAEXAMPLE", "autoStopHours": 4, "useCliCreds": true, "monthlyLimit": 25.0 }
  ]
}"#,
    );
    data.write(
        "storage.json",
        r#"{ "images": [ { "reference": "nginx:1.27", "id": "sha256:aa" }, { "reference": "kalilinux/kali-rolling", "id": "sha256:bb" } ],
             "boxes": [ { "name": "generic/debian12", "provider": "virtualbox", "version": "4.3.12" } ] }"#,
    );
    data.write("shared-folder.json", r#"{ "enabled": true, "path": "/home/player/CyberCTF/shared" }"#);
    data.write(
        "installed-tools.json",
        r#"[ { "dependency": "vagrant", "at": 1700000000 }, { "dependency": "virtualbox", "at": 1700000100, "pending": true } ]"#,
    );
    data.write(
        "reap-errors/goat",
        r#"{"message":"Auto-stop couldn't destroy goat: the token has expired","error":{"code":"other","message":"the token has expired"}}"#,
    );
    data.write("reap-errors/not a lab", "{}");
}

#[test]
fn a_data_folder_from_before_the_store_is_imported_once_and_its_files_left_as_they_were() {
    let data = Data::new("import");
    fixture(&data);
    let before = std::fs::read_to_string(data.0.join("server.json")).unwrap();
    let store = data.store();
    let (default, saved) = store.read(|c| Ok((hosts::default(c)?, hosts::all(c)?))).unwrap();
    assert_eq!(default.as_deref(), Some("pve1"));
    assert_eq!(saved.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["pve1", "esx", "aws1"], "in the order they were saved");
    assert!(saved[0].insecure_tls && saved[2].use_cli_creds);
    assert_eq!((saved[2].auto_stop_hours, saved[2].monthly_limit), (Some(4), Some(25.0)));
    let owned = store.read(storage::load).unwrap();
    assert_eq!((owned.images.len(), owned.boxes.len()), (2, 1));
    let shared: crate::shared_folder::SharedFolder = store.read(|c| settings::get(c, settings::SHARED_FOLDER)).unwrap().unwrap();
    assert_eq!((shared.enabled, shared.path.as_deref()), (true, Some("/home/player/CyberCTF/shared")));
    let recorded = store.read(tools::all).unwrap();
    assert_eq!(
        recorded.iter().map(|t| (t.dependency, t.at, t.pending)).collect::<Vec<_>>(),
        [(Dependency::Vagrant, 1_700_000_000, false), (Dependency::Virtualbox, 1_700_000_100, true)]
    );
    assert_eq!(store.read(autostop::failed).unwrap(), ["goat"], "a name that isn't a lab id is left out");
    assert_eq!(store.read(|c| autostop::last(c, "goat")).unwrap().unwrap().message, "the token has expired");
    assert_eq!(std::fs::read_to_string(data.0.join("server.json")).unwrap(), before, "the file stays as it was");

    // Changed in the store, then started again: the files are never imported a second time.
    store.write(|tx| hosts::replace(tx, None, &[])).unwrap();
    let store = data.restart();
    assert!(store.read(hosts::all).unwrap().is_empty(), "a removed host doesn't come back from the old file");
    assert_eq!(store.read(storage::load).unwrap().images.len(), 2, "nothing doubled");
}

#[test]
fn a_file_that_cant_be_read_is_imported_at_a_later_start_beside_what_was_saved_since() {
    let data = Data::new("retry");
    data.write("server.json", "{ not json");
    data.write("installed-tools.json", r#"[ { "dependency": "docker", "at": 5 } ]"#);
    let store = data.store();
    assert!(store.read(hosts::all).unwrap().is_empty(), "nothing taken from a damaged file");
    assert_eq!(store.read(tools::all).unwrap().len(), 1, "the other files are imported");
    let added = crate::runtime::server::profile::HostProfile {
        id: "new".into(),
        ..serde_json::from_str(r#"{ "id": "x", "name": "New", "provider": "proxmox", "host": "h", "port": 8006, "username": "u" }"#).unwrap()
    };
    store.write(|tx| hosts::replace(tx, None, std::slice::from_ref(&added))).unwrap();
    // The player put the file right; the next start takes it, keeping what was saved since.
    data.write(
        "server.json",
        r#"{ "default": "old", "hosts": [ { "id": "old", "name": "Old", "provider": "proxmox", "host": "h", "port": 8006, "username": "u" } ] }"#,
    );
    let store = data.restart();
    let saved = store.read(hosts::all).unwrap();
    assert_eq!(saved.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["new", "old"]);
    assert_eq!(store.read(hosts::default).unwrap().as_deref(), Some("old"));
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
    store.write(|tx| autostop::record(tx, "a", "m", &w)).unwrap();
    let failed = store.write(|tx| -> crate::error::Result<()> {
        autostop::replace(tx, &[])?;
        autostop::record(tx, "b", "m", &w)?;
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
