//! Answers of the tools that rarely change (`vagrant --version`, `vagrant plugin list`), kept a
//! while instead of asked on every machine check: each is a process start, on Windows a Ruby one
//! of a second or two of CPU, and the Machine page checks the machine again whenever it is shown.
//! Only an answer that found the tool is kept: one installed since is found at the next check.
//! Installing or removing a tool from the app forgets them all ([`forget_all`]).

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How long a kept answer stands: a tool changed outside the app shows up after this at most.
pub const KEPT_FOR: Duration = Duration::from_secs(600);

/// The kept answers, by key, with when each was asked.
fn kept() -> MutexGuard<'static, BTreeMap<String, (Instant, String)>> {
    static KEPT: Mutex<BTreeMap<String, (Instant, String)>> = Mutex::new(BTreeMap::new());
    KEPT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The answer kept for `key` if it is younger than `KEPT_FOR`, else `ask`'s, kept when it is `Ok`.
///
/// # Errors
///
/// `ask`'s, which isn't kept: the next call asks again.
pub async fn remembered<E>(key: &str, ask: impl Future<Output = Result<String, E>>) -> Result<String, E> {
    // Unit tests script each tool's answers per test: one test's must never stand for another's.
    if cfg!(test) {
        return ask.await;
    }
    remembered_at(key, Instant::now(), ask).await
}

/// [`remembered`] at time `now`.
async fn remembered_at<E>(key: &str, now: Instant, ask: impl Future<Output = Result<String, E>>) -> Result<String, E> {
    if let Some((at, answer)) = kept().get(key)
        && now.saturating_duration_since(*at) < KEPT_FOR
    {
        return Ok(answer.clone());
    }
    let answer = ask.await?;
    kept().insert(key.to_string(), (now, answer.clone()));
    Ok(answer)
}

/// Forgets every kept answer: a tool was installed or removed.
pub fn forget_all() {
    kept().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_answer_is_kept_a_while_a_failure_never() {
        let key = format!("test-{}", rand::random::<u32>());
        let start = Instant::now();
        let asked = std::cell::Cell::new(0);
        let ask = |answer: Result<&'static str, ()>| {
            let asked = &asked;
            async move {
                asked.set(asked.get() + 1);
                answer.map(str::to_string)
            }
        };
        assert_eq!(remembered_at(&key, start, ask(Err(()))).await, Err(()));
        assert_eq!(remembered_at(&key, start, ask(Ok("Vagrant 2.4.9"))).await.unwrap(), "Vagrant 2.4.9");
        assert_eq!(asked.get(), 2, "a failure isn't kept: asked again");
        let later = start + KEPT_FOR.saturating_sub(Duration::from_secs(1));
        assert_eq!(remembered_at(&key, later, ask(Err(()))).await.unwrap(), "Vagrant 2.4.9");
        assert_eq!(asked.get(), 2, "the kept answer stood: not asked");
        assert_eq!(remembered_at(&key, start + KEPT_FOR, ask(Ok("Vagrant 2.5.0"))).await.unwrap(), "Vagrant 2.5.0", "asked again once old");
        forget_all();
        assert_eq!(remembered_at(&key, start + KEPT_FOR, ask(Ok("Vagrant 2.5.1"))).await.unwrap(), "Vagrant 2.5.1", "forgotten after an install");
    }
}
