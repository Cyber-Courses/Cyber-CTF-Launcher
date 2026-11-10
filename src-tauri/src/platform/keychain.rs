//! The OS keychain (the macOS Keychain, Windows' Credential Manager, the Secret Service on Linux):
//! one entry per name under the app's service. One rule for an entry that isn't there (a read
//! finds none, a delete is done already), and one error for a keychain that fails, which says what
//! to do on Linux, where it fails most. A failure is never read as "none": the sign-in session and
//! the servers' secrets both keep that rule.
//!
//! A call blocks: the OS may show its unlock prompt and wait for the player.

use crate::error::{Error, Result};
use crate::platform::config::KEYCHAIN_SERVICE;

fn entry(name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, name).map_err(|e| failed(&e))
}

/// The value kept under `name`; none when there is no such entry.
pub fn get(name: &str) -> Result<Option<String>> {
    read(entry(name)?.get_password())
}

/// Keeps `value` under `name`.
pub fn set(name: &str, value: &str) -> Result<()> {
    entry(name)?.set_password(value).map_err(|e| failed(&e))
}

/// Deletes the entry `name`; one that isn't there is deleted already.
pub fn delete(name: &str) -> Result<()> {
    removed(entry(name)?.delete_credential())
}

/// What a read found: the value, none for an entry that isn't there, else why the keychain failed.
fn read(answer: keyring::Result<String>) -> Result<Option<String>> {
    match answer {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(failed(&e)),
    }
}

/// The outcome of a delete: gone (or never there), or why it stays.
fn removed(answer: keyring::Result<()>) -> Result<()> {
    match answer {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(failed(&e)),
    }
}

/// The keychain failing is the common Linux case: no Secret Service (GNOME Keyring, KWallet)
/// running, or it is locked. Says what to do; the raw error stays in parentheses for support.
fn failed(e: &keyring::Error) -> Error {
    Error::Invalid(format!(
        "Cyber CTF couldn't use the system keychain. On Linux, start and unlock a keyring (GNOME Keyring or KWallet), then try again. ({e})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_missing_entry_reads_as_none_and_a_failure_says_what_to_do() {
        assert_eq!(read(Ok("pw".into())).unwrap().as_deref(), Some("pw"));
        assert_eq!(read(Err(keyring::Error::NoEntry)).unwrap(), None);
        let locked = read(Err(keyring::Error::NoStorageAccess("the keychain is locked".into()))).unwrap_err().to_string();
        // Regression: the servers' secrets said "keychain: …" alone; only the session's store
        // said how to unlock a keyring.
        assert!(locked.contains("On Linux, start and unlock a keyring") && locked.ends_with("the keychain is locked)"), "{locked}");
        assert!(removed(Ok(())).is_ok() && removed(Err(keyring::Error::NoEntry)).is_ok());
        let refused = removed(Err(keyring::Error::PlatformFailure("the keychain is locked".into()))).unwrap_err();
        assert!(refused.to_string().contains("the keychain is locked"), "{refused}");
    }
}
