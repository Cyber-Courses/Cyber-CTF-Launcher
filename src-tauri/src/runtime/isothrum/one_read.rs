//! One status read of a run at a time in the app. The lab page polls a lab's status, the attack
//! box's status polls it too (the box is one of the lab's machines), and a read right after an
//! operation, or the attack box's auto-start at the end of a launch, doesn't wait for a poll: each
//! asked the runner on its own, so two `vagrant status` of the same VMs could run at once. Every
//! read of a VM's state takes its lock in Vagrant's machine index without waiting
//! (`MachineLocked`), so Vagrant refused one of them: the lab page said it couldn't read the lab,
//! or the attack box "couldn't be added with the lab" (seen on libvirt). The reads of one run now
//! take turns, and a read that waited for one that began after it was asked for takes that one's
//! answer: as fresh as its own would have been, without asking Vagrant again.
//!
//! The runner itself asks no tool while an operation holds the run, in this process or the deploy
//! worker ([`isothrum_runner::Status::live`]): this is about the reads between operations.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use isothrum_runner::Status;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::Result;

/// The order reads are asked for and begin in, across every run: a read may stand for another
/// asked for before it began.
static TICKS: AtomicU64 = AtomicU64::new(0);

/// The next tick.
fn tick() -> u64 {
    TICKS.fetch_add(1, Ordering::Relaxed)
}

/// A run's last read: the tick it began at, and its answer.
type Last = Option<(u64, Status)>;

/// The turn of run `key`'s reads, holding its last read.
fn turn(key: &Path) -> Arc<AsyncMutex<Last>> {
    static TURNS: Mutex<BTreeMap<PathBuf, Arc<AsyncMutex<Last>>>> = Mutex::new(BTreeMap::new());
    TURNS.lock().unwrap_or_else(PoisonError::into_inner).entry(key.to_path_buf()).or_default().clone()
}

/// Run `key`'s status, from `read` when its turn comes, or from the read that ended meanwhile when
/// that one began after this was asked for. A failed read stands for no other.
pub(super) async fn status<F>(key: &Path, read: impl FnOnce() -> F) -> Result<Status>
where
    F: Future<Output = Result<Status>>,
{
    let asked = tick();
    let turn = turn(key);
    let mut last = turn.lock().await;
    if let Some((began, status)) = last.as_ref().filter(|(began, _)| *began > asked) {
        log::debug!("{}: the status read at tick {began} stands for the one asked at {asked}", key.display());
        return Ok(status.clone());
    }
    let began = tick();
    let status = read().await?;
    *last = Some((began, status.clone()));
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Status {
        serde_json::from_value(serde_json::json!({ "state": "ready", "provider": null, "machines": [], "networks": [], "outputs": [], "message": null }))
            .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn reads_of_a_run_take_turns_and_one_asked_for_meanwhile_takes_the_next_ones_answer() {
        let key = std::env::temp_dir().join(format!("one-read-{}", rand::random::<u32>()));
        let reads = AtomicU64::new(0);
        let (running, most) = (AtomicU64::new(0), AtomicU64::new(0));
        let read = || async {
            reads.fetch_add(1, Ordering::SeqCst);
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(ready())
        };
        // A poll in flight, then two more reads asked for while it runs.
        let (a, b, c) = tokio::join!(status(&key, read), status(&key, read), status(&key, read));
        assert!(a.is_ok() && b.is_ok() && c.is_ok());
        assert_eq!(most.load(Ordering::SeqCst), 1, "never two at once");
        // The first read began before the others were asked for: the second reads again, and the
        // third, asked for before that one began, takes its answer.
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        // Asked for after every read ended: a read of its own.
        status(&key, read).await.unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_failed_read_stands_for_no_other() {
        let key = std::env::temp_dir().join(format!("one-read-{}", rand::random::<u32>()));
        let failed = status(&key, || async { Err(crate::error::Error::Invalid("refused".into())) }).await;
        assert!(failed.is_err());
        assert!(status(&key, || async { Ok(ready()) }).await.is_ok());
    }
}
