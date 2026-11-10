//! Each host's secret (password, API token or access key): the OS keychain in release builds,
//! a 0600 JSON file in debug builds, since every `tauri dev` rebuild is a new unsigned binary
//! and the keychain would re-prompt on each run (as the sign-in session's store does). Both
//! stores are built in every build, so each is checked and tested; [`IN_A_FILE`] picks one.
//!
//! A keychain call blocks: the OS may show its unlock prompt and wait for the player. Called
//! from an async worker, it runs through [`blocking`], so the worker's other tasks move on to
//! another thread meanwhile.

use super::profile::HostProfile;
use crate::error::{Code, Error, Result};
use crate::platform::keychain;

/// Whether this build keeps the secrets in a file (a dev build) rather than the keychain.
const IN_A_FILE: bool = cfg!(debug_assertions);

/// The secret stored for `id`. None stored is [`Code::NoPasswordStored`]; a store that can't be
/// read (a locked keychain, its unlock prompt dismissed, a secrets file that doesn't parse) says
/// why instead.
pub(crate) fn get_secret(id: &str) -> Result<String> {
    if IN_A_FILE { found(dev_file::read()?.remove(id)) } else { blocking(|| keychain::get(&entry_name(id))).and_then(found) }
}

pub(crate) fn set_secret(id: &str, secret: &str) -> Result<()> {
    if IN_A_FILE {
        let mut map = dev_file::read()?;
        map.insert(id.to_string(), secret.to_string());
        dev_file::write(&map)
    } else {
        blocking(|| keychain::set(&entry_name(id), secret))
    }
}

/// Deletes the secret stored for `id`; one that isn't there is deleted already. A store that
/// refuses (a locked keychain, its prompt dismissed) says so.
pub(crate) fn delete_secret(id: &str) -> Result<()> {
    if IN_A_FILE {
        let mut map = dev_file::read()?;
        if map.remove(id).is_some() {
            dev_file::write(&map)?;
        }
        Ok(())
    } else {
        blocking(|| keychain::delete(&entry_name(id)))
    }
}

/// The secret a host connects with: empty for hosts that sign in through their own CLI (see
/// `HostProfile::keeps_secret`), else the stored one.
pub(crate) fn host_secret(host: &HostProfile) -> Result<String> {
    if host.keeps_secret() { get_secret(&host.id) } else { Ok(String::new()) }
}

/// The secret stored for `id`, or the reason there is none.
fn found(secret: Option<String>) -> Result<String> {
    secret.ok_or_else(|| Error::coded(Code::NoPasswordStored))
}

/// Runs `call`, which blocks, so that it doesn't hold up an async worker: on a multi-threaded
/// runtime's worker the worker's other tasks are handed to another thread first
/// ([`tokio::task::block_in_place`]). Elsewhere (a plain thread, a single-threaded runtime) it
/// just runs. The callers stay synchronous: a launch's connection is read deep in code that is.
fn blocking<T>(call: impl FnOnce() -> T) -> T {
    let on_a_worker = tokio::runtime::Handle::try_current().is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
    if on_a_worker { tokio::task::block_in_place(call) } else { call() }
}

/// The release builds' store: one keychain entry per host, `server:<id>`.
fn entry_name(id: &str) -> String {
    format!("server:{id}")
}

/// The dev builds' store: every host's secret in one JSON file under the home folder.
mod dev_file {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use crate::error::{Error, Result};

    #[cfg(test)]
    thread_local! {
        /// A test's own secrets file (see `use_test_file`), so it never writes the player's.
        pub(super) static TEST_FILE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    }

    fn path() -> Result<PathBuf> {
        #[cfg(test)]
        if let Some(file) = TEST_FILE.with(|f| f.borrow().clone()) {
            return Ok(file);
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).ok_or_else(|| Error::Invalid("no home directory".into()))?;
        Ok(file_in(&PathBuf::from(home)))
    }

    /// The secrets file under a home folder.
    pub(super) fn file_in(home: &Path) -> PathBuf {
        home.join(".cyberctf").join("dev-server-secrets.json")
    }

    pub(super) fn read() -> Result<BTreeMap<String, String>> {
        read_at(&path()?)
    }

    /// The secrets in `path`: none when it isn't there. A file that can't be read or doesn't parse
    /// is an error, never "no secrets": the next save would write the one secret over all the others.
    pub(super) fn read_at(path: &Path) -> Result<BTreeMap<String, String>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(Error::Invalid(format!("couldn't read the secrets file {}: {e}", path.display()))),
        };
        serde_json::from_str(&raw).map_err(|e| Error::Invalid(format!("the secrets file {} isn't readable: {e}", path.display())))
    }

    pub(super) fn write(map: &BTreeMap<String, String>) -> Result<()> {
        write_at(&path()?, map)
    }

    /// Writes the secrets to `path` whole (a crash leaves the previous file), the file and its
    /// folder this user's only from the start.
    pub(super) fn write_at(path: &Path, map: &BTreeMap<String, String>) -> Result<()> {
        if let Some(dir) = path.parent() {
            crate::platform::private_fs::dir(dir)?;
        }
        let raw = serde_json::to_string(map).map_err(|e| Error::Invalid(e.to_string()))?;
        crate::platform::atomic_file::write_private(path, raw)?;
        Ok(())
    }
}

/// Keeps this thread's secrets in `file` instead of the player's (a test that saves one).
#[cfg(test)]
pub(crate) fn use_test_file(file: std::path::PathBuf) {
    dev_file::TEST_FILE.with(|f| *f.borrow_mut() = Some(file));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::providers::Provider;
    use crate::runtime::server::profile::tests::profile;

    #[test]
    fn a_missing_secret_says_to_enter_one() {
        assert_eq!(found(Some("pw".into())).unwrap(), "pw");
        assert_eq!(found(None).unwrap_err().code(), Code::NoPasswordStored);
    }

    #[test]
    fn a_keychain_that_cant_be_read_says_so_and_never_reads_as_no_secret() {
        // Regression: a locked keychain (or its unlock prompt dismissed) read as "no password
        // stored", so the player was told to enter the host's password again.
        assert_eq!(Ok(Some("pw".to_string())).and_then(found).unwrap(), "pw");
        assert_eq!(Ok(None).and_then(found).unwrap_err().code(), Code::NoPasswordStored);
        let locked: Result<Option<String>> = Err(Error::Invalid("the keychain is locked".into()));
        assert_ne!(locked.and_then(found).unwrap_err().code(), Code::NoPasswordStored);
        assert_eq!(entry_name("pve1"), "server:pve1");
    }

    #[test]
    fn cli_signed_in_hosts_need_no_stored_secret() {
        // Nothing is read for them: the empty secret comes back without touching any store.
        for p in [Provider::Azure, Provider::Gcp, Provider::Oci] {
            assert_eq!(host_secret(&profile(p)).unwrap(), "");
        }
        let cli = HostProfile { use_cli_creds: true, ..profile(Provider::Aws) };
        assert_eq!(host_secret(&cli).unwrap(), "");
    }

    /// A fresh home folder for a test's secrets file.
    fn home() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cyberctf-secrets-{}", rand::random::<u32>()))
    }

    #[test]
    fn the_dev_secrets_file_round_trips_owner_only() {
        use std::collections::BTreeMap;
        let home = home();
        let file = dev_file::file_in(&home);
        assert!(file.ends_with(std::path::Path::new(".cyberctf").join("dev-server-secrets.json")));
        assert!(dev_file::read_at(&file).unwrap().is_empty(), "no file: no secrets");
        let mut map = BTreeMap::new();
        map.insert("ab12".to_string(), "s3cret".to_string());
        dev_file::write_at(&file, &map).unwrap();
        assert_eq!(dev_file::read_at(&file).unwrap(), map);
        #[cfg(unix)]
        crate::platform::private_fs::tests::assert_private_with_folder(&file);
        let names: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["dev-server-secrets.json"], "written whole, no temporary file left");
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_secrets_file_that_doesnt_parse_is_an_error_and_is_never_written_over() {
        // Regression: it read as no secrets, so saving one host's secret erased every other's.
        let home = home();
        let file = dev_file::file_in(&home);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{\"ab12\": \"s3cret\", cut sho").unwrap();
        use_test_file(file.clone());
        let err = dev_file::read_at(&file).unwrap_err().to_string();
        assert!(err.starts_with("the secrets file") && err.contains("isn't readable"), "{err}");
        assert!(set_secret("cd34", "other").is_err());
        assert!(get_secret("ab12").unwrap_err().code() != Code::NoPasswordStored, "not \"none stored\"");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"ab12\": \"s3cret\", cut sho", "left as it was");
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_blocking_call_hands_the_workers_other_tasks_on() {
        // One worker: without the hand-over, the task the call waits for could never run.
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let waited = rt.block_on(async {
            tokio::spawn(async {
                let (tx, rx) = std::sync::mpsc::channel();
                tokio::spawn(async move { tx.send(()).unwrap() });
                blocking(|| rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok())
            })
            .await
            .unwrap()
        });
        assert!(waited, "the other task ran while the call blocked");
        // Off a worker (a single-threaded runtime, a plain thread) it just runs.
        assert_eq!(tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async { blocking(|| 7) }), 7);
        assert_eq!(blocking(|| 8), 8);
    }
}
