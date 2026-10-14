//! Background downloads of the test image and box.
//!
//! Setup starts the test downloads as soon as it opens (`machine_selftest_prefetch`), so by
//! the time a test runs its image/box is usually there. A download in flight is marked busy;
//! a test that needs it waits for it instead of starting a second one.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::docker::IMAGE;
use super::vm::pick_box;
use crate::error::Result;
use crate::exec::{run, run_env_timed, stream};
use crate::runtime::providers::Provider;

static DOCKER_DL: AtomicBool = AtomicBool::new(false);
static VM_DL: AtomicBool = AtomicBool::new(false);
/// The test image is 2 MB: past this the engine is stuck, not the network slow.
const PULL_LIMIT: Duration = Duration::from_secs(300);
/// Last line of the VM box download, so a test waiting on it can show progress.
static VM_DL_LINE: Mutex<String> = Mutex::new(String::new());

/// Holds a download flag; releases it when dropped (also on error).
struct Busy(&'static AtomicBool);

impl Busy {
    fn try_take(flag: &'static AtomicBool) -> Option<Busy> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Busy(flag))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Makes sure the test image is local. Returns whether it already was (or a background
/// download finished it) rather than being pulled now.
pub(super) async fn ensure_image() -> Result<bool> {
    let mut waited = false;
    loop {
        if run("docker", &["image", "inspect", IMAGE], None).await.is_ok() {
            return Ok(true);
        }
        if let Some(_busy) = Busy::try_take(&DOCKER_DL) {
            run_env_timed("docker", &["pull", "-q", IMAGE], None, &[], PULL_LIMIT).await?;
            return Ok(waited);
        }
        waited = true;
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

/// Makes sure the test box is local. `on_line` sees download progress, including from a
/// background download this call is waiting on. Returns whether it was already there.
pub(super) async fn ensure_box(dir: &Path, p: Provider, bx: &'static str, arm: bool, mut on_line: impl FnMut(String)) -> Result<bool> {
    let mut waited = false;
    loop {
        if pick_box(p, arm).await == (bx, true) {
            return Ok(true);
        }
        if let Some(_busy) = Busy::try_take(&VM_DL) {
            on_line(format!("Downloading {bx} for {}, once…", p.id()));
            stream("vagrant", &["box", "add", bx, "--provider", p.id(), "--force"], Some(dir), &[], |l| {
                if let Ok(mut last) = VM_DL_LINE.lock() {
                    last.clone_from(&l);
                }
                on_line(l);
            })
            .await?;
            return Ok(waited);
        }
        waited = true;
        let line = VM_DL_LINE.lock().map(|l| l.clone()).unwrap_or_default();
        on_line(if line.trim().is_empty() { format!("Finishing the background download of {bx}…") } else { line });
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_flag_is_taken_once_and_released_on_drop() {
        static FLAG: AtomicBool = AtomicBool::new(false);
        let first = Busy::try_take(&FLAG);
        assert!(first.is_some());
        assert!(Busy::try_take(&FLAG).is_none());
        drop(first);
        assert!(Busy::try_take(&FLAG).is_some());
    }
}
