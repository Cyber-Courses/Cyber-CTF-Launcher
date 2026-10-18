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
use crate::platform::exec::{run, run_env_timed, stream};
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

    #[tokio::test]
    async fn ensure_image_pulls_only_when_the_test_image_is_missing() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        // Already local: `image inspect` answers, so nothing is pulled and it reports "was there".
        let present = Tools::rules(&[("image inspect", ok("sha256:abc"))]);
        assert!(ensure_image().await.unwrap(), "already local");
        assert!(!present.called("pull"), "nothing pulled: {:?}", present.calls());
        drop(present);
        // Missing: `image inspect` fails, the image is pulled quietly, and this call did the pull.
        let pulled = Tools::rules(&[("image inspect", fail("Error: No such image")), ("pull", ok(""))]);
        assert!(!ensure_image().await.unwrap(), "pulled now, not already there");
        assert!(pulled.called("pull -q"), "{:?}", pulled.calls());
        drop(pulled);
        // The pull itself fails (a wedged engine): surfaced as an error, not a false "ready".
        let _failed = Tools::rules(&[("image inspect", fail("Error: No such image")), ("pull", fail("timed out"))]);
        assert!(ensure_image().await.is_err());
    }

    #[tokio::test]
    async fn ensure_box_adds_a_missing_box_and_forwards_its_progress() {
        use crate::platform::exec::fake::{Tools, ok};
        let dir = std::env::temp_dir().join(format!("cyberctf-dl-box-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        // `box list` shows nothing, so the box is added once; the lines Vagrant prints, and the
        // "Downloading …" notice, reach on_line.
        let tools = Tools::rules(&[("box list", ok("")), ("box add", ok("Progress: 10%\nProgress: 100%"))]);
        let mut lines = Vec::new();
        let was_there = ensure_box(&dir, Provider::Virtualbox, "bento/ubuntu-24.04", false, |l| lines.push(l)).await.unwrap();
        assert!(!was_there, "it had to be downloaded");
        assert!(tools.called("box add bento/ubuntu-24.04 --provider virtualbox"), "{:?}", tools.calls());
        assert!(lines.iter().any(|l| l.contains("Downloading bento/ubuntu-24.04 for")), "the notice: {lines:?}");
        assert!(lines.iter().any(|l| l == "Progress: 100%"), "the streamed lines: {lines:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
