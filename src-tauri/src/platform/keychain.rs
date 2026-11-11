//! The OS keychain (the macOS Keychain, Windows' Credential Manager, the Secret Service on Linux):
//! one entry per name under the app's service. One rule for an entry that isn't there (a read
//! finds none, a delete is done already, as it is where there is no keychain at all), and one
//! error for a keychain that fails, which says what
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

/// Deletes the entry `name`; one that isn't there, or no keychain here at all, is deleted already.
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

/// The outcome of a delete: gone, never there, or no keychain here at all (nothing can be kept in
/// one that isn't there); else why it stays.
fn removed(answer: keyring::Result<()>) -> Result<()> {
    match answer {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) if no_keychain(&e) => Ok(()),
        Err(e) => Err(failed(&e)),
    }
}

/// What the Secret Service backend's error says when this computer has no keychain at all: no
/// provider answers on the session bus (`ServiceUnknown`), or there is no session bus. A locked
/// keychain is another error (`NoStorageAccess`), and so is any other failure.
const NO_KEYCHAIN: [&str; 3] = ["org.freedesktop.DBus.Error.ServiceUnknown", "Unable to autolaunch a dbus-daemon", "No DBus session"];

/// Whether `e` says there is no keychain here at all (Linux without a Secret Service provider).
fn no_keychain(e: &keyring::Error) -> bool {
    let keyring::Error::PlatformFailure(cause) = e else { return false };
    let said = format!("{cause} {cause:?}");
    NO_KEYCHAIN.iter().any(|m| said.contains(m))
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

    #[test]
    fn deleting_where_there_is_no_keychain_at_all_is_done_and_a_locked_one_says_why() {
        // Linux without a Secret Service provider, or without a session bus: nothing can be kept,
        // so nothing is left to delete.
        for missing in [
            "Unexpected Error: Dbus(D-Bus error: The name org.freedesktop.secrets was not provided by any .service files (org.freedesktop.DBus.Error.ServiceUnknown))",
            "Unexpected Error: Dbus(D-Bus error: Unable to autolaunch a dbus-daemon without a $DISPLAY for X11 (org.freedesktop.DBus.Error.NotSupported))",
            "No DBus session or Secret Service provider found",
        ] {
            assert!(removed(Err(keyring::Error::PlatformFailure(missing.into()))).is_ok(), "{missing}");
        }
        // A keychain that is there and refuses: why, never "deleted".
        assert!(removed(Err(keyring::Error::NoStorageAccess("Secret Service: object locked".into()))).is_err());
        assert!(removed(Err(keyring::Error::PlatformFailure("Unexpected Error: Parse".into()))).is_err());
        // Only a delete reads it so: a read from no keychain still fails, never reads as none.
        let read_missing = read(Err(keyring::Error::PlatformFailure("org.freedesktop.DBus.Error.ServiceUnknown".into())));
        assert!(read_missing.is_err());
    }
}
