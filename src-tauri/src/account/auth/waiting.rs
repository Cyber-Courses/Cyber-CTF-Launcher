//! The sign-in waiting for the browser: one at a time, and cancellable. A browser that accepts
//! the sign-in link and then shows nothing can't be detected, so the player can always cancel
//! instead of waiting out the timeout; a second sign-in (from another window) ends the first.

use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::oneshot;

use crate::error::{Code, Error, Result};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// The sign-in not finished within [`LOGIN_TIMEOUT`].
fn login_timed_out() -> Error {
    Error::coded(Code::LoginTimeout { minutes: LOGIN_TIMEOUT.as_secs() / 60 })
}

/// The sign-in waiting now: its number, and the sender that cancels it.
static WAITING: Mutex<Option<(u64, oneshot::Sender<()>)>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

fn waiting() -> std::sync::MutexGuard<'static, Option<(u64, oneshot::Sender<()>)>> {
    WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Removes sign-in `id` from the waiting one, if it still is: nothing can cancel it any more.
fn forget(id: u64) {
    let mut current = waiting();
    if current.as_ref().is_some_and(|(waiting_id, _)| *waiting_id == id) {
        *current = None;
    }
}

/// A sign-in that may wait for the browser. Dropped, it is no longer the one waiting.
pub(super) struct Waiting {
    id: u64,
    cancelled: oneshot::Receiver<()>,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        forget(self.id);
    }
}

/// Ends a sign-in's cancellability once its code is being exchanged: a cancel then would drop
/// a session already on its way to the keychain, and report "cancelled" for a sign-in that went
/// through.
#[derive(Clone, Copy)]
pub(super) struct Disarm(u64);

impl Disarm {
    pub(super) fn now(self) {
        forget(self.0);
    }
}

/// Starts a sign-in's wait. One already waiting ends now, as cancelled.
pub(super) fn begin() -> Waiting {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, cancelled) = oneshot::channel();
    if let Some((_, earlier)) = waiting().replace((id, tx)) {
        let _ = earlier.send(());
    }
    Waiting { id, cancelled }
}

/// Ends the sign-in waiting now, if any, as cancelled.
pub(super) fn cancel() {
    if let Some((_, waiting)) = waiting().take() {
        let _ = waiting.send(());
    }
}

impl Waiting {
    /// What ends this sign-in's cancellability (see [`Disarm`]).
    pub(super) fn disarm(&self) -> Disarm {
        Disarm(self.id)
    }

    /// `wait`'s outcome, unless the sign-in is cancelled (or a newer one starts) first, or it
    /// takes longer than the timeout.
    pub(super) async fn run<T>(self, wait: impl Future<Output = Result<T>>) -> Result<T> {
        self.run_within(LOGIN_TIMEOUT, wait).await
    }

    async fn run_within<T>(mut self, timeout: Duration, wait: impl Future<Output = Result<T>>) -> Result<T> {
        // A sender dropped without a word (disarmed, or this sign-in is done) is no cancel.
        let cancelled = async {
            if (&mut self.cancelled).await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            done = tokio::time::timeout(timeout, wait) => done.unwrap_or_else(|_| Err(login_timed_out())),
            () = cancelled => Err(Error::Cancelled),
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// The tests over the one waiting sign-in run one at a time.
    pub(in crate::account::auth) static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn a_cancelled_wait_ends_as_cancelled_and_a_finished_one_with_its_outcome() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let wait = begin();
        let run = tokio::spawn(wait.run(std::future::pending::<Result<()>>()));
        cancel();
        assert!(matches!(run.await.unwrap(), Err(Error::Cancelled)));
        assert_eq!(begin().run(async { Ok(7) }).await.unwrap(), 7);
        // Done: nothing is left waiting, and a cancel then is a no-op.
        assert!(waiting().is_none());
        cancel();
    }

    #[tokio::test]
    async fn a_disarmed_sign_in_is_no_longer_cancelled() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let wait = begin();
        let disarm = wait.disarm();
        let (finish, finished) = oneshot::channel::<()>();
        let run = tokio::spawn(wait.run(async move {
            let _ = finished.await;
            Ok("kept")
        }));
        disarm.now();
        cancel();
        finish.send(()).unwrap();
        assert_eq!(run.await.unwrap().unwrap(), "kept");
    }

    #[test]
    fn a_sign_in_that_fails_before_waiting_leaves_nothing_to_cancel() {
        let _serial = ONE_AT_A_TIME.blocking_lock();
        drop(begin());
        assert!(waiting().is_none());
    }

    #[tokio::test]
    async fn a_second_sign_in_ends_the_first() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let first = tokio::spawn(begin().run(std::future::pending::<Result<()>>()));
        let (release, released) = oneshot::channel::<()>();
        let second = tokio::spawn(begin().run(async move {
            let _ = released.await;
            Ok("second")
        }));
        assert!(matches!(first.await.unwrap(), Err(Error::Cancelled)));
        // The first one ending doesn't clear the second's registration.
        assert!(waiting().is_some());
        release.send(()).unwrap();
        assert_eq!(second.await.unwrap().unwrap(), "second");
        assert!(waiting().is_none());
    }

    #[tokio::test]
    async fn a_wait_past_the_timeout_says_so() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let err = begin().run_within(Duration::from_millis(10), std::future::pending::<Result<()>>()).await.unwrap_err();
        assert_eq!(err.code(), Code::LoginTimeout { minutes: 5 });
    }
}
