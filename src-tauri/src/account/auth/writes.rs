//! The session's writes, one at a time. Every write of the session (a refresh, a sign-in, a
//! sign-out) goes through a [`SessionWrite`], which holds the write lock for as long as it lives:
//! the writes are its methods, so none is made without the lock.
//! - At start-up the agent and the first API calls all find the same expired token and would
//!   each send the same refresh token. With refresh-token rotation the second one is rejected
//!   (`invalid_grant`), and that used to wipe the session. Callers queue here and re-read the
//!   session once they hold the lock, so the first refresh serves them all.
//! - A refresh in flight can't write its renewed session back over a sign-out (the player would
//!   be signed in again at the next call) or over another account's sign-in.

use super::store::{self, Session};
use crate::error::Result;

/// The lock every write of the session holds (see the module).
static SESSION_WRITE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A renewed session the keychain refused to save. Refresh tokens rotate: the old one in the
/// keychain is spent, so dropping the new one would sign the player out at the next call (an
/// `invalid_grant`). It is served from here, and saved again at each read until that works.
#[cfg(not(test))]
static UNSAVED: std::sync::Mutex<Option<Session>> = std::sync::Mutex::new(None);
#[cfg(test)]
thread_local! {
    /// Per test thread, like the session file (`store::dev_session_path`).
    static UNSAVED: std::sync::Mutex<Option<Session>> = const { std::sync::Mutex::new(None) };
}

/// `f` on the renewed session waiting to be saved.
fn with_unsaved<T>(f: impl FnOnce(&mut Option<Session>) -> T) -> T {
    let run = |m: &std::sync::Mutex<Option<Session>>| f(&mut m.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    #[cfg(test)]
    return UNSAVED.with(run);
    #[cfg(not(test))]
    run(&UNSAVED)
}

/// The renewed session waiting to be saved, if any: a read, free of the lock.
pub(super) fn unsaved() -> Option<Session> {
    with_unsaved(|u| u.clone())
}

/// `SESSION_WRITE`, held until this is dropped: what writes the session (the renewed one waiting
/// to be saved included) takes it, so a write outside the lock doesn't compile.
pub(super) struct SessionWrite {
    /// Held for its lock, never read.
    _held: tokio::sync::MutexGuard<'static, ()>,
}

#[allow(clippy::unused_self, reason = "the guard is the proof the write lock is held: its writes take it")]
impl SessionWrite {
    /// Waits for the other writes of the session to finish, then holds the lock.
    pub(super) async fn lock() -> SessionWrite {
        SessionWrite { _held: SESSION_WRITE.lock().await }
    }

    /// Keeps `session` as the renewed session waiting to be saved (`None`: none).
    pub(super) fn set_unsaved(&self, session: Option<Session>) {
        with_unsaved(|u| *u = session);
    }

    /// Saves `session` in the keychain (the session file, in a dev build).
    pub(super) async fn store(&self, session: Session) -> Result<()> {
        store::store(session).await
    }

    /// Forgets the stored session.
    pub(super) async fn clear(&self) -> Result<()> {
        store::clear().await
    }

    /// The session calls use: the stored one, unless a newer renewed one is still waiting to be
    /// saved (then that one, and saving is tried again).
    pub(super) async fn current_session(&self) -> Result<Session> {
        let stored = super::session_or_signed_out().await;
        let Some(renewed) = unsaved() else { return stored };
        match stored {
            // Signed out (or unreadable) since: the renewed session is not resurrected.
            Err(e) => {
                self.set_unsaved(None);
                Err(e)
            }
            Ok(s) if renewed.expires_at <= s.expires_at => {
                self.set_unsaved(None);
                Ok(s)
            }
            Ok(_) => {
                if self.store(renewed.clone()).await.is_ok() {
                    self.set_unsaved(None);
                }
                Ok(renewed)
            }
        }
    }
}
