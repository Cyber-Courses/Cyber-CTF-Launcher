//! Where the session is kept.
//!
//! Release builds keep the session in the OS keychain. Dev builds keep it in a file
//! instead: every `tauri dev` rebuild is an unsigned, different binary, so the keychain
//! re-prompts for access on each run ("...wants to use your confidential information").
//! A file avoids that churn while developing. (A signed release build, with a stable code
//! signature + keychain-access-group, prompts at most once.)

use serde::{Deserialize, Serialize};

#[cfg(not(debug_assertions))]
use crate::app::config;
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
    entry_named("session")
}

#[cfg(not(debug_assertions))]
fn entry_named(name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(config::KEYCHAIN_SERVICE, name).map_err(keychain_error)
}

/// The most a keychain entry holds here, in UTF-16 units: Windows' Credential Manager caps a
/// credential at 2560 bytes (1280 UTF-16 units). A session (a JWT access token, a refresh token,
/// the profile) can be longer, and saving it failed, so it was never kept. A longer session is
/// split across `session.1`, `session.2`… with `session` holding how many there are.
#[cfg_attr(debug_assertions, allow(dead_code))]
const CHUNK_UNITS: usize = 1200;
/// At most this many pieces (a session is a few KiB; some identity providers issue tokens of
/// several KiB each). A longer one is refused at save, never half-saved and read as signed out.
#[cfg_attr(debug_assertions, allow(dead_code))]
const MAX_CHUNKS: usize = 32;
#[cfg_attr(debug_assertions, allow(dead_code))]
const CHUNKS_PREFIX: &str = "cyberctf-chunks:";

/// What goes in the `session` entry and, for a long session, the pieces for `session.1`….
#[cfg_attr(debug_assertions, allow(dead_code))]
fn split_chunks(raw: &str) -> Result<(String, Vec<String>)> {
    if raw.encode_utf16().count() <= CHUNK_UNITS {
        return Ok((raw.to_string(), Vec::new()));
    }
    let mut parts = vec![String::new()];
    let mut units = 0;
    for c in raw.chars() {
        if units + c.len_utf16() > CHUNK_UNITS {
            parts.push(String::new());
            units = 0;
        }
        units += c.len_utf16();
        if let Some(p) = parts.last_mut() {
            p.push(c);
        }
    }
    if parts.len() > MAX_CHUNKS {
        return Err(Error::Invalid(
            "Your session is too large to keep in the system keychain. Sign in again; if this keeps happening, contact support.".into(),
        ));
    }
    Ok((format!("{CHUNKS_PREFIX}{}", parts.len()), parts))
}

/// The session's JSON from the `session` entry's value, reading the pieces with `piece(i)`
/// (1-based) when it was split. None when a piece is missing.
#[cfg_attr(debug_assertions, allow(dead_code))]
fn join_chunks(head: String, mut piece: impl FnMut(usize) -> Option<String>) -> Option<String> {
    let Some(n) = head.strip_prefix(CHUNKS_PREFIX) else { return Some(head) };
    let n: usize = n.parse().ok().filter(|n| (1..=MAX_CHUNKS).contains(n))?;
    (1..=n).map(&mut piece).collect()
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
    // Unit tests never read or replace the developer's own session: theirs is per test run.
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("cyberctf-test-session-{}", std::process::id())).join("dev-session.json"));
    }
    home_session_path()
}

/// The dev session file in the home folder.
#[cfg(debug_assertions)]
fn home_session_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(std::path::PathBuf::from(home).join(".cyberctf").join("dev-session.json"))
}

/// The session kept in a dev session file, if it is there and parses.
#[cfg(debug_assertions)]
fn load_file(path: &std::path::Path) -> Option<Session> {
    std::fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Writes the session to a dev session file, creating its folder: both this user's only (the
/// file holds the session's tokens).
#[cfg(debug_assertions)]
fn save_file(path: &std::path::Path, raw: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        crate::platform::private_fs::dir(dir)?;
    }
    crate::platform::atomic_file::write_private(path, raw)?;
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
        let piece = |i: usize| entry_named(&format!("session.{i}")).ok()?.get_password().ok();
        let read = entry().map_err(|e| e.to_string()).and_then(|e| match e.get_password() {
            Ok(head) => Ok(join_chunks(head, piece).and_then(|raw| serde_json::from_str(&raw).ok())),
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
        // The pieces first, then the head naming them: a reader never sees a head whose pieces
        // aren't there yet. Pieces left from a longer session before are removed.
        let (head, parts) = split_chunks(&raw)?;
        for (i, part) in parts.iter().enumerate() {
            entry_named(&format!("session.{}", i + 1))?.set_password(part).map_err(keychain_error)?;
        }
        entry()?.set_password(&head).map_err(keychain_error)?;
        clear_chunks_from(parts.len() + 1);
        backoff::set(None);
        Ok(())
    }
}

/// Removes the session pieces `session.<from>` and after.
#[cfg(not(debug_assertions))]
fn clear_chunks_from(from: usize) {
    for i in from..=MAX_CHUNKS {
        if let Ok(e) = entry_named(&format!("session.{i}")) {
            let _ = e.delete_credential();
        }
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
        clear_chunks_from(1);
    }
}

/// Forgets the session, on a blocking thread.
pub(super) async fn clear() {
    let _ = tokio::task::spawn_blocking(clear_blocking).await;
}

#[cfg(test)]
mod chunk_tests {
    use super::*;

    #[test]
    fn a_session_too_long_for_one_windows_credential_is_split_and_joined_back() {
        // Windows refuses a credential over 1280 UTF-16 units: a long session was never kept.
        let short = r#"{"access_token":"a"}"#;
        assert_eq!(split_chunks(short).unwrap(), (short.to_string(), Vec::new()));
        assert_eq!(join_chunks(short.into(), |_| None).as_deref(), Some(short));

        let long = format!(r#"{{"access_token":"{}","name":"Zoë 🦀{}"}}"#, "x".repeat(3000), "é".repeat(700));
        let (head, parts) = split_chunks(&long).unwrap();
        assert_eq!(head, format!("cyberctf-chunks:{}", parts.len()));
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.encode_utf16().count() <= CHUNK_UNITS));
        assert!(head.encode_utf16().count() <= CHUNK_UNITS);
        assert_eq!(join_chunks(head.clone(), |i| parts.get(i - 1).cloned()).as_deref(), Some(long.as_str()));
        // A missing piece reads as no session, never as a cut one.
        assert_eq!(join_chunks(head, |i| (i != 2).then(|| parts[i - 1].clone())), None);
        assert_eq!(join_chunks("cyberctf-chunks:999".into(), |_| Some(String::new())), None);
    }

    #[test]
    fn a_session_of_exactly_one_credential_is_kept_whole_and_one_unit_more_is_split() {
        let exact = "a".repeat(CHUNK_UNITS);
        assert_eq!(split_chunks(&exact).unwrap(), (exact.clone(), Vec::new()));
        let over = format!("{exact}b");
        let (head, parts) = split_chunks(&over).unwrap();
        assert_eq!(head, "cyberctf-chunks:2");
        assert_eq!((parts[0].len(), parts[1].as_str()), (CHUNK_UNITS, "b"));
        assert_eq!(join_chunks(head, |i| parts.get(i - 1).cloned()).as_deref(), Some(over.as_str()));
        // Counts that aren't pieces: zero, text, and a bare prefix.
        for head in ["cyberctf-chunks:0", "cyberctf-chunks:two", "cyberctf-chunks:"] {
            assert_eq!(join_chunks(head.into(), |_| Some("x".into())), None, "{head}");
        }
    }

    #[test]
    fn a_two_unit_character_never_straddles_a_piece_boundary() {
        // 1199 units, then a character of two UTF-16 units: it moves whole to the next piece
        // (a credential holding half a surrogate pair would not be a valid string on Windows).
        let raw = format!("{}\u{1f980}b", "a".repeat(CHUNK_UNITS - 1));
        let (head, parts) = split_chunks(&raw).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].encode_utf16().count(), CHUNK_UNITS - 1);
        assert!(parts[1].starts_with('\u{1f980}'));
        assert_eq!(join_chunks(head, |i| parts.get(i - 1).cloned()).as_deref(), Some(raw.as_str()));
    }

    #[test]
    fn a_session_with_more_pieces_than_the_read_accepts_is_refused_at_save() {
        // `save_blocking` wrote every piece and a head naming them all, `load_blocking` refused
        // the count and reported no session: signed out right after a sign-in that said it worked.
        let raw = format!(r#"{{"access_token":"{}"}}"#, "x".repeat(CHUNK_UNITS * MAX_CHUNKS));
        assert!(split_chunks(&raw).unwrap_err().to_string().contains("too large"));
        // The largest session that fits still round-trips.
        let fits = "y".repeat(CHUNK_UNITS * MAX_CHUNKS);
        let (head, parts) = split_chunks(&fits).unwrap();
        assert_eq!(parts.len(), MAX_CHUNKS);
        assert_eq!(join_chunks(head, |i| parts.get(i - 1).cloned()).as_deref(), Some(fits.as_str()));
    }
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
        #[cfg(unix)]
        {
            assert_eq!(crate::platform::private_fs::tests::mode(&path), 0o600);
            assert_eq!(crate::platform::private_fs::tests::mode(path.parent().unwrap()), 0o700);
        }
        let back = load_file(&path).unwrap();
        assert_eq!((back.access_token.as_str(), back.refresh_token.as_deref(), back.expires_at), ("a", Some("r"), 42));
        assert_eq!(back.name.as_deref(), Some("N"));
        std::fs::write(&path, "not json").unwrap();
        assert!(load_file(&path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_dev_session_lives_in_the_home_folder_but_not_in_tests() {
        if let Some(p) = home_session_path() {
            assert!(p.ends_with(std::path::Path::new(".cyberctf").join("dev-session.json")));
            assert_ne!(dev_session_path(), Some(p));
        }
        ask_keychain_again();
    }
}

#[cfg(test)]
mod proptests;
