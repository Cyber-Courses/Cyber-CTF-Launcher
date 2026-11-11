//! Where the session is kept.
//!
//! Release builds keep the session in the OS keychain. Dev builds keep it in a file
//! instead: every `tauri dev` rebuild is an unsigned, different binary, so the keychain
//! re-prompts for access on each run ("...wants to use your confidential information").
//! A file avoids that churn while developing. (A signed release build, with a stable code
//! signature + keychain-access-group, prompts at most once.) Both stores are built in every
//! build, so each is checked and tested; [`in_a_file`] picks one: a test, whatever profile built
//! it, keeps the session in a file too, never in the system's keychain.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::platform::keychain;

#[derive(Serialize, Deserialize, Clone)]
pub(super) struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Seconds since epoch.
    pub expires_at: u64,
    pub name: Option<String>,
    pub email: Option<String>,
    /// The account's picture (the ID token's `picture` claim); none for a session kept before
    /// the launcher read it, or an account without one.
    #[serde(default)]
    pub picture: Option<String>,
}

/// Whether this build keeps the session in a file (a dev build, a test build whatever its
/// profile, or an end-to-end run's app, `platform::test_root`) rather than the keychain: a test
/// binary built with a release-like profile (the rig's `rig`) has no debug assertions.
fn in_a_file() -> bool {
    cfg!(debug_assertions) || cfg!(test) || crate::platform::test_root::root().is_some()
}

/// The keychain entry holding the session, or its head when it is split (see [`split_chunks`]).
const SESSION: &str = "session";

/// The keychain entry of the session's piece `i` (1-based).
fn piece_name(i: usize) -> String {
    format!("session.{i}")
}

/// The most a keychain entry holds here, in UTF-16 units: Windows' Credential Manager caps a
/// credential at 2560 bytes (1280 UTF-16 units). A session (a JWT access token, a refresh token,
/// the profile) can be longer, and saving it failed, so it was never kept. A longer session is
/// split across `session.1`, `session.2`… with `session` holding how many there are.
const CHUNK_UNITS: usize = 1200;
/// At most this many pieces (a session is a few KiB; some identity providers issue tokens of
/// several KiB each). A longer one is refused at save, never half-saved and read as signed out.
const MAX_CHUNKS: usize = 32;
const CHUNKS_PREFIX: &str = "cyberctf-chunks:";

/// What goes in the `session` entry and, for a long session, the pieces for `session.1`….
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
fn join_chunks(head: String, mut piece: impl FnMut(usize) -> Option<String>) -> Option<String> {
    let Some(n) = head.strip_prefix(CHUNKS_PREFIX) else { return Some(head) };
    let n: usize = n.parse().ok().filter(|n| (1..=MAX_CHUNKS).contains(n))?;
    (1..=n).map(&mut piece).collect()
}

/// The session a keychain `head` holds: joined from its pieces (`piece(i)`, 1-based: `Ok(None)`
/// for one that isn't there) when it was split, then parsed. None when a piece is missing or the
/// text isn't a session (another app's entry, a save cut short): that reads as signed out, never
/// as a keychain error, and the next sign-in writes over it. A piece the keychain couldn't read is
/// that error: the session is there, unreadable for now.
fn decode(head: String, mut piece: impl FnMut(usize) -> std::result::Result<Option<String>, String>) -> std::result::Result<Option<Session>, String> {
    let mut failed = None;
    let raw = join_chunks(head, |i| {
        piece(i).unwrap_or_else(|why| {
            failed.get_or_insert(why);
            None
        })
    });
    match failed {
        Some(why) => Err(why),
        None => Ok(raw.and_then(|raw| session_of(&raw))),
    }
}

pub(super) fn dev_session_path() -> Option<std::path::PathBuf> {
    // Unit tests never read or replace the developer's own session, nor each other's: each test
    // thread has its own, removed when the thread ends (a `#[tokio::test]` runs its tasks on its
    // one thread, and the file is resolved there, before any blocking thread, see `session_file`).
    #[cfg(test)]
    let path = Some(crate::testing::thread_dir("test-session").join("dev-session.json"));
    #[cfg(not(test))]
    let path = home_session_path();
    path
}

/// The dev session file a read, save or clear goes to (none when the keychain keeps the
/// session), resolved on the caller's thread.
fn session_file() -> Option<std::path::PathBuf> {
    if in_a_file() { dev_session_path() } else { None }
}

/// The dev session file in the home folder (an end-to-end run's, in one).
fn home_session_path() -> Option<std::path::PathBuf> {
    Some(crate::platform::own_home()?.join(".cyberctf").join("dev-session.json"))
}

/// The session `raw` holds; none when it isn't one this version reads (a launcher update changed
/// the session's fields, a save cut short), which signs the player out: logged, so support can
/// tell that sign-out from one the player asked for. Only where the text stops being a session is
/// said, never the text (it holds the tokens).
fn session_of(raw: &str) -> Option<Session> {
    serde_json::from_str(raw)
        .inspect_err(|e| {
            log::warn!("the stored session isn't one this version reads, so you're signed out: {:?} at line {}, column {}", e.classify(), e.line(), e.column());
        })
        .ok()
}

/// The session kept in a dev session file: none when there is no file or it doesn't parse (as a
/// keychain entry that isn't a session), an error when it can't be read.
fn load_file(path: &std::path::Path) -> std::result::Result<Option<Session>, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(session_of(&raw)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("the session file {} can't be read: {e}", path.display())),
    }
}

/// Writes the session to a dev session file, creating its folder: both this user's only (the
/// file holds the session's tokens).
fn save_file(path: &std::path::Path, raw: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        crate::platform::private_fs::dir(dir)?;
    }
    crate::platform::atomic_file::write_private(path, raw)?;
    Ok(())
}

/// The stored session: `Ok(None)` when there is none, `Err` when the keychain couldn't be read.
/// Blocking (the Secret Service may show its unlock prompt and wait): call `read`.
fn load_blocking(file: Option<&std::path::Path>) -> std::result::Result<Option<Session>, String> {
    if in_a_file() { file.map_or(Ok(None), load_file) } else { load_keychain() }
}

/// The session in the keychain, unless the keychain failed a moment ago (see [`backoff`]).
fn load_keychain() -> std::result::Result<Option<Session>, String> {
    if let Some(why) = backoff::active() {
        return Err(why);
    }
    let piece = |i: usize| keychain::get(&piece_name(i)).map_err(|e| e.to_string());
    let read = match keychain::get(SESSION) {
        Ok(Some(head)) => decode(head, piece),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    };
    if let Err(why) = &read {
        backoff::set(Some(why.clone()));
    }
    read
}

/// After the keychain fails (locked, its unlock prompt dismissed, no Secret Service), it isn't
/// asked again for a minute: the background agent reads the session every few seconds, and each
/// read would raise the unlock prompt again right after the player dismissed it. Signing in
/// clears it.
mod backoff {
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

    static KEYCHAIN_BACKOFF: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    const KEYCHAIN_RETRY: Duration = Duration::from_secs(60);

    /// Why the keychain failed, while it isn't to be asked again.
    pub(super) fn active() -> Option<String> {
        let guard = KEYCHAIN_BACKOFF.lock().unwrap_or_else(PoisonError::into_inner);
        guard.as_ref().filter(|(at, _)| at.elapsed() < KEYCHAIN_RETRY).map(|(_, why)| why.clone())
    }

    pub(super) fn set(why: Option<String>) {
        *KEYCHAIN_BACKOFF.lock().unwrap_or_else(PoisonError::into_inner) = why.map(|w| (Instant::now(), w));
    }
}

/// A fresh sign-in asks the keychain again, even right after it failed.
pub(super) fn ask_keychain_again() {
    if !in_a_file() {
        backoff::set(None);
    }
}

/// `load_blocking` on a blocking thread, never the UI thread or an async worker.
pub(super) async fn read() -> std::result::Result<Option<Session>, String> {
    let file = session_file();
    tokio::task::spawn_blocking(move || load_blocking(file.as_deref())).await.unwrap_or_else(|e| Err(e.to_string()))
}

fn save_blocking(session: &Session, file: Option<&std::path::Path>) -> Result<()> {
    let raw = serde_json::to_string(session).map_err(|e| Error::Invalid(e.to_string()))?;
    if in_a_file() {
        let path = file.ok_or_else(|| Error::Invalid("no home directory".into()))?;
        return save_file(path, &raw);
    }
    // The pieces first, then the head naming them: a reader never sees a head whose pieces
    // aren't there yet. Pieces left from a longer session before are removed.
    let (head, parts) = split_chunks(&raw)?;
    for (i, part) in parts.iter().enumerate() {
        keychain::set(&piece_name(i + 1), part)?;
    }
    keychain::set(SESSION, &head)?;
    // The head names how many pieces to read: one left from before is never read, only stale.
    if let Err(e) = clear_chunks_from(parts.len() + 1) {
        log::warn!("the session is saved, but a piece of the one before stays in the keychain: {e}");
    }
    backoff::set(None);
    Ok(())
}

/// Removes the session pieces `session.<from>` and after, every one tried; the first failure.
fn clear_chunks_from(from: usize) -> Result<()> {
    (from..=MAX_CHUNKS).map(|i| keychain::delete(&piece_name(i))).fold(Ok(()), Result::and)
}

/// `save_blocking` on a blocking thread (the keychain may prompt).
pub(super) async fn store(session: Session) -> Result<()> {
    let file = session_file();
    tokio::task::spawn_blocking(move || save_blocking(&session, file.as_deref())).await.map_err(|e| Error::Invalid(e.to_string()))?
}

fn clear_blocking(file: Option<&std::path::Path>) -> Result<()> {
    if in_a_file() {
        return file.map_or(Ok(()), remove_file);
    }
    // The head first: without it the pieces are never read, so the session is gone even if one
    // of them stays.
    keychain::delete(SESSION)?;
    clear_chunks_from(1)
}

/// Removes the dev build's session file; one that isn't there is removed already.
fn remove_file(path: &std::path::Path) -> Result<()> {
    crate::platform::fs::remove_if_present(path).map_err(|e| Error::Invalid(format!("couldn't remove {}: {e}", path.display())))
}

/// Forgets the session, on a blocking thread.
///
/// # Errors
///
/// When the keychain (the session file, in a dev build) still holds it.
pub(super) async fn clear() -> Result<()> {
    let file = session_file();
    tokio::task::spawn_blocking(move || clear_blocking(file.as_deref())).await.map_err(|e| Error::Invalid(e.to_string()))?
}

#[cfg(test)]
mod chunk_tests {
    use super::*;

    #[test]
    fn a_test_keeps_the_session_in_a_file_whatever_profile_built_it() {
        // `cargo test --profile rig` (a release) has no debug assertions: the keychain, before.
        assert!(in_a_file(), "a test reaches the system keychain");
        assert!(session_file().is_some());
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dev_session_file_round_trips_and_a_bad_one_reads_as_none() {
        let dir = std::env::temp_dir().join(format!("cyberctf-session-{}", rand::random::<u64>()));
        let path = dir.join("nested").join("dev-session.json");
        assert!(matches!(load_file(&path), Ok(None)));
        let session = Session { access_token: "a".into(), refresh_token: Some("r".into()), expires_at: 42, name: Some("N".into()), email: None, picture: None };
        save_file(&path, &serde_json::to_string(&session).unwrap()).unwrap();
        crate::platform::private_fs::tests::assert_private_with_folder(&path);
        let back = load_file(&path).unwrap().unwrap();
        assert_eq!((back.access_token.as_str(), back.refresh_token.as_deref(), back.expires_at), ("a", Some("r"), 42));
        assert_eq!(back.name.as_deref(), Some("N"));
        // Regression: a session that doesn't parse signed the player out with nothing logged.
        std::fs::write(&path, r#"{"access_token":"secret-token","expires_at":"soon"}"#).unwrap();
        crate::testing::logs::take_warnings();
        assert!(matches!(load_file(&path), Ok(None)));
        let warned = crate::testing::logs::take_warnings();
        assert!(
            warned.len() == 1 && warned[0].starts_with("the stored session isn't one this version reads, so you're signed out: Data at line 1"),
            "{warned:?}"
        );
        assert!(!warned[0].contains("secret-token") && !warned[0].contains("soon"), "never the session's text: {warned:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_session_is_forgotten_only_when_its_store_lets_it_go() {
        // Regression: every failure to delete was dropped, and sign-out said it worked while the
        // keychain kept the session.
        // (A keychain delete's rule is platform::keychain's.)
        let dir = std::env::temp_dir().join(format!("cyberctf-clear-{}", rand::random::<u64>()));
        let path = dir.join("dev-session.json");
        assert!(remove_file(&path).is_ok(), "no file: already signed out");
        std::fs::create_dir_all(&path).unwrap();
        assert!(remove_file(&path).is_err(), "a session file that can't be removed is said");
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, "{}").unwrap();
        remove_file(&path).unwrap();
        assert!(!path.exists());
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

    #[test]
    fn a_corrupt_keychain_session_reads_as_no_session_not_as_an_error() {
        // The entry holds text that isn't a session (another app's, a save cut short), a session
        // with a field of the wrong type, or names pieces one of which is gone: no session, so the
        // app shows signed out without a keychain error, and a fresh sign-in replaces it.
        // Whether a session was read, or the error.
        let read = |r: std::result::Result<Option<Session>, String>| r.map(|s| s.is_some());
        assert_eq!(read(decode("not json".into(), |_| Ok(None))), Ok(false));
        assert_eq!(read(decode(r#"{"access_token":42,"expires_at":1}"#.into(), |_| Ok(None))), Ok(false));
        let long = Session { access_token: "x".repeat(3000), refresh_token: Some("r".into()), expires_at: 42, name: None, email: None, picture: None };
        let (head, parts) = split_chunks(&serde_json::to_string(&long).unwrap()).unwrap();
        assert!(parts.len() >= 2);
        assert_eq!(read(decode(head.clone(), |i| Ok((i != 2).then(|| parts[i - 1].clone())))), Ok(false), "a missing piece");
        assert_eq!(read(decode(head.clone(), |i| Ok(parts.get(i - 1).map(|p| p.replace('x', "\""))))), Ok(false), "a garbled piece");
        let back = decode(head.clone(), |i| Ok(parts.get(i - 1).cloned())).unwrap().unwrap();
        assert_eq!((back.access_token.len(), back.refresh_token.as_deref(), back.expires_at), (3000, Some("r"), 42));
        // Regression: a piece the keychain couldn't read (locked, say) read as missing, so as
        // signed out. It is the keychain's error.
        let locked = decode(head, |i| if i == 2 { Err("the keychain is locked".into()) } else { Ok(parts.get(i - 1).cloned()) });
        assert_eq!(read(locked), Err("the keychain is locked".to_string()));
    }

    #[test]
    fn a_dev_session_file_that_cant_be_read_is_an_error_not_signed_out() {
        let dir = crate::testing::temp_dir("session-file");
        let path = dir.join("dev-session.json");
        assert!(matches!(load_file(&path), Ok(None)), "no file");
        std::fs::write(&path, "not a session").unwrap();
        assert!(matches!(load_file(&path), Ok(None)));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir_all(path.join("held")).unwrap();
        assert!(load_file(&path).err().is_some_and(|e| e.starts_with("the session file ")));
        std::fs::remove_dir_all(dir).ok();
    }
}

#[cfg(test)]
mod proptests;
