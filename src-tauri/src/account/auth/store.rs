//! Where the session is kept.
//!
//! Release builds keep the session in the OS keychain. Dev builds keep it in a file
//! instead: every `tauri dev` rebuild is an unsigned, different binary, so the keychain
//! re-prompts for access on each run ("...wants to use your confidential information").
//! A file avoids that churn while developing. (A signed release build, with a stable code
//! signature + keychain-access-group, prompts at most once.)

use serde::{Deserialize, Serialize};

#[cfg(not(debug_assertions))]
use crate::config;
use crate::error::{Error, Result};

#[derive(Serialize, Deserialize, Clone)]
pub(super) struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Seconds since epoch.
    pub expires_at: u64,
    pub name: Option<String>,
    pub email: Option<String>,
}

#[cfg(not(debug_assertions))]
fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new(config::KEYCHAIN_SERVICE, "session").map_err(keychain_error)
}

/// The keychain failing is the common Linux case: no Secret Service (GNOME Keyring, KWallet)
/// running, or it is locked. Say what to do; the raw error stays in parentheses for support.
#[cfg(not(debug_assertions))]
fn keychain_error(e: keyring::Error) -> Error {
    Error::Invalid(format!(
        "Cyber CTF couldn't use the system keychain to keep your session. On Linux, start and unlock a keyring \
         (GNOME Keyring or KWallet) and sign in again. ({e})"
    ))
}

#[cfg(debug_assertions)]
fn dev_session_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(std::path::PathBuf::from(home).join(".cyberctf").join("dev-session.json"))
}

/// The session kept in a dev session file, if it is there and parses.
#[cfg(debug_assertions)]
fn load_file(path: &std::path::Path) -> Option<Session> {
    std::fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Writes the session to a dev session file, creating its folder.
#[cfg(debug_assertions)]
fn save_file(path: &std::path::Path, raw: &str) -> Result<()> {
    crate::atomic_file::write(path, raw)?;
    Ok(())
}

/// The stored session: `Ok(None)` when there is none, `Err` when the keychain couldn't be read.
/// Blocking (the Secret Service may show its unlock prompt and wait): call `read`.
fn load_blocking() -> std::result::Result<Option<Session>, String> {
    #[cfg(debug_assertions)]
    {
        let Some(path) = dev_session_path() else { return Ok(None) };
        Ok(load_file(&path))
    }
    #[cfg(not(debug_assertions))]
    {
        if let Some(why) = backoff::active() {
            return Err(why);
        }
        let read = entry().map_err(|e| e.to_string()).and_then(|e| match e.get_password() {
            Ok(raw) => Ok(serde_json::from_str(&raw).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(e).to_string()),
        });
        if let Err(why) = &read {
            backoff::set(Some(why.clone()));
        }
        read
    }
}

/// After the keychain fails (locked, its unlock prompt dismissed, no Secret Service), it isn't
/// asked again for a minute: the background agent reads the session every few seconds, and each
/// read would raise the unlock prompt again right after the player dismissed it. Signing in
/// clears it.
#[cfg(not(debug_assertions))]
mod backoff {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static KEYCHAIN_BACKOFF: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    const KEYCHAIN_RETRY: Duration = Duration::from_secs(60);

    /// Why the keychain failed, while it isn't to be asked again.
    pub(super) fn active() -> Option<String> {
        let guard = KEYCHAIN_BACKOFF.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().filter(|(at, _)| at.elapsed() < KEYCHAIN_RETRY).map(|(_, why)| why.clone())
    }

    pub(super) fn set(why: Option<String>) {
        *KEYCHAIN_BACKOFF.lock().unwrap_or_else(|e| e.into_inner()) = why.map(|w| (Instant::now(), w));
    }
}

/// A fresh sign-in asks the keychain again, even right after it failed.
pub(super) fn ask_keychain_again() {
    #[cfg(not(debug_assertions))]
    backoff::set(None);
}

/// `load_blocking` on a blocking thread, never the UI thread or an async worker.
pub(super) async fn read() -> std::result::Result<Option<Session>, String> {
    tokio::task::spawn_blocking(load_blocking).await.unwrap_or_else(|e| Err(e.to_string()))
}

fn save_blocking(session: &Session) -> Result<()> {
    let raw = serde_json::to_string(session).map_err(|e| Error::Invalid(e.to_string()))?;
    #[cfg(debug_assertions)]
    {
        let path = dev_session_path().ok_or_else(|| Error::Invalid("no home directory".into()))?;
        save_file(&path, &raw)
    }
    #[cfg(not(debug_assertions))]
    {
        entry()?.set_password(&raw).map_err(keychain_error)?;
        backoff::set(None);
        Ok(())
    }
}

/// `save_blocking` on a blocking thread (the keychain may prompt).
pub(super) async fn store(session: Session) -> Result<()> {
    tokio::task::spawn_blocking(move || save_blocking(&session)).await.map_err(|e| Error::Invalid(e.to_string()))?
}

fn clear_blocking() {
    #[cfg(debug_assertions)]
    {
        if let Some(path) = dev_session_path() {
            let _ = std::fs::remove_file(path);
        }
    }
    #[cfg(not(debug_assertions))]
    {
        if let Ok(e) = entry() {
            let _ = e.delete_credential();
        }
    }
}

/// Forgets the session, on a blocking thread.
pub(super) async fn clear() {
    let _ = tokio::task::spawn_blocking(clear_blocking).await;
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    #[test]
    fn a_dev_session_file_round_trips_and_a_bad_one_reads_as_none() {
        let dir = std::env::temp_dir().join(format!("cyberctf-session-{}", rand::random::<u64>()));
        let path = dir.join("nested").join("dev-session.json");
        assert!(load_file(&path).is_none());
        let session = Session { access_token: "a".into(), refresh_token: Some("r".into()), expires_at: 42, name: Some("N".into()), email: None };
        save_file(&path, &serde_json::to_string(&session).unwrap()).unwrap();
        let back = load_file(&path).unwrap();
        assert_eq!((back.access_token.as_str(), back.refresh_token.as_deref(), back.expires_at), ("a", Some("r"), 42));
        assert_eq!(back.name.as_deref(), Some("N"));
        std::fs::write(&path, "not json").unwrap();
        assert!(load_file(&path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_dev_session_lives_in_the_home_folder() {
        if let Some(p) = dev_session_path() {
            assert!(p.ends_with(std::path::Path::new(".cyberctf").join("dev-session.json")));
        }
        ask_keychain_again();
    }
}
