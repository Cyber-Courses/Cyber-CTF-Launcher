//! Each host's secret (password, API token or access key): the OS keychain in release builds,
//! a 0600 JSON file in debug builds, since every `tauri dev` rebuild is a new unsigned binary
//! and the keychain would re-prompt on each run (same as auth.rs).

use super::profile::HostProfile;
use crate::error::{Code, Error, Result};

#[cfg(not(debug_assertions))]
fn entry(id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(crate::app::config::KEYCHAIN_SERVICE, &format!("server:{id}")).map_err(|e| Error::Invalid(format!("keychain: {e}")))
}

#[cfg(debug_assertions)]
mod dev_file {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use crate::error::{Error, Result};

    fn path() -> Result<PathBuf> {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).ok_or_else(|| Error::Invalid("no home directory".into()))?;
        Ok(file_in(&PathBuf::from(home)))
    }

    /// The secrets file under a home folder.
    pub(super) fn file_in(home: &Path) -> PathBuf {
        home.join(".cyberctf").join("dev-server-secrets.json")
    }

    pub fn read() -> BTreeMap<String, String> {
        path().ok().map(|p| read_at(&p)).unwrap_or_default()
    }

    /// The secrets in `path`; none when it is missing or unreadable.
    pub(super) fn read_at(path: &Path) -> BTreeMap<String, String> {
        std::fs::read_to_string(path).ok().and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default()
    }

    pub fn write(map: &BTreeMap<String, String>) -> Result<()> {
        write_at(&path()?, map)
    }

    /// Writes the secrets to `path` (its folder created), readable by this user only.
    pub(super) fn write_at(path: &Path, map: &BTreeMap<String, String>) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string(map).map_err(|e| Error::Invalid(e.to_string()))?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

/// The secret stored for `id`, or the reason there is none.
fn found(secret: Option<String>) -> Result<String> {
    secret.ok_or_else(|| Error::coded(Code::NoPasswordStored))
}

pub(super) fn get_secret(id: &str) -> Result<String> {
    #[cfg(debug_assertions)]
    let secret = dev_file::read().remove(id);
    #[cfg(not(debug_assertions))]
    let secret = entry(id)?.get_password().ok();
    found(secret)
}

pub(super) fn set_secret(id: &str, secret: &str) -> Result<()> {
    #[cfg(debug_assertions)]
    {
        let mut map = dev_file::read();
        map.insert(id.to_string(), secret.to_string());
        dev_file::write(&map)
    }
    #[cfg(not(debug_assertions))]
    {
        entry(id)?.set_password(secret).map_err(|e| Error::Invalid(format!("keychain: {e}")))
    }
}

pub(super) fn delete_secret(id: &str) {
    #[cfg(debug_assertions)]
    {
        let mut map = dev_file::read();
        if map.remove(id).is_some() {
            let _ = dev_file::write(&map);
        }
    }
    #[cfg(not(debug_assertions))]
    {
        if let Ok(e) = entry(id) {
            let _ = e.delete_credential();
        }
    }
}

/// The secret a host connects with: empty for hosts that sign in through their own CLI (see
/// `HostProfile::keeps_secret`), else the stored one.
pub(super) fn host_secret(host: &HostProfile) -> Result<String> {
    if host.keeps_secret() { get_secret(&host.id) } else { Ok(String::new()) }
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
    fn cli_signed_in_hosts_need_no_stored_secret() {
        // Nothing is read for them: the empty secret comes back without touching any store.
        for p in [Provider::Azure, Provider::Gcp, Provider::Oci] {
            assert_eq!(host_secret(&profile(p)).unwrap(), "");
        }
        let cli = HostProfile { use_cli_creds: true, ..profile(Provider::Aws) };
        assert_eq!(host_secret(&cli).unwrap(), "");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_dev_secrets_file_round_trips() {
        use std::collections::BTreeMap;
        let home = std::env::temp_dir().join(format!("cyberctf-secrets-{}", rand::random::<u32>()));
        let file = dev_file::file_in(&home);
        assert!(file.ends_with(std::path::Path::new(".cyberctf").join("dev-server-secrets.json")));
        // Missing, then garbage: both read as no secrets.
        assert!(dev_file::read_at(&file).is_empty());
        let mut map = BTreeMap::new();
        map.insert("ab12".to_string(), "s3cret".to_string());
        dev_file::write_at(&file, &map).unwrap();
        assert_eq!(dev_file::read_at(&file), map);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&file, "not json").unwrap();
        assert!(dev_file::read_at(&file).is_empty());
        std::fs::remove_dir_all(home).unwrap();
    }
}
