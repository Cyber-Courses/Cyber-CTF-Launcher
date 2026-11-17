//! What a self-test downloaded, added to the runner's record of downloads so Storage lists and
//! cleans it: the self-tests still run their tools themselves, until they run through the runner
//! (which records its own downloads). Each image (by the reference it pulled and the image id it
//! got) and box version that appeared during the self-test, among those it uses, is adopted:
//! one the player had already is never touched.
//!
//! The record's shape (`Owned`/`OwnedImage`/`OwnedBox`, what earlier launchers kept), its
//! `vagrant box list` parser are in [`record`].

use std::collections::BTreeSet;

mod record;

use isothrum_runner::DownloadToAdopt;
use record::parse_box_list;
pub use record::{Owned, OwnedBox, OwnedImage, listed_boxes};

use crate::platform::AppDirs;
use crate::platform::exec::run_read;

/// What was on this machine before a self-test that may download: the image ids and the boxes,
/// each only when asked for and when Docker or Vagrant could say (`None`: nothing is claimed).
struct Before {
    images: Option<BTreeSet<String>>,
    boxes: Option<BTreeSet<OwnedBox>>,
}

impl Before {
    /// Reads what is on this machine now: the image ids when `images`, the boxes when `boxes`.
    async fn take(images: bool, boxes: bool) -> Before {
        let (images, boxes) = tokio::join!(async { if images { image_ids().await } else { None } }, async { if boxes { box_list().await } else { None } });
        Before { images, boxes }
    }
}

/// Every image id on this machine; `None` when Docker can't say.
async fn image_ids() -> Option<BTreeSet<String>> {
    let out = run_read("docker", &["image", "ls", "-q", "--no-trunc"], None).await.ok()?;
    Some(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// The image `reference` names now.
async fn image_id(reference: &str) -> Option<String> {
    let out = run_read("docker", &["image", "inspect", "--format", "{{.Id}}", reference], None).await.ok()?;
    Some(out.trim().to_string()).filter(|id| !id.is_empty())
}

/// Every box version on this machine; `None` when Vagrant can't say.
async fn box_list() -> Option<BTreeSet<OwnedBox>> {
    run_read("vagrant", &["box", "list"], None).await.ok().map(|out| parse_box_list(&out))
}

/// What appeared since `before` among what the self-test used: the images of `images`
/// (references) and the box versions named in `boxes`, as the runner adopts them.
async fn appeared(before: &Before, images: &[String], boxes: &[String]) -> Vec<DownloadToAdopt> {
    let mut new = Vec::new();
    if let Some(had) = &before.images {
        for reference in images {
            if let Some(id) = image_id(reference).await.filter(|id| !had.contains(id)) {
                new.push(DownloadToAdopt::Image { reference: reference.clone(), image_id: Some(id) });
            }
        }
    }
    if let (Some(had), false) = (&before.boxes, boxes.is_empty())
        && let Some(now) = box_list().await
    {
        let added = now.difference(had).filter(|b| boxes.contains(&b.name));
        new.extend(added.map(|b| DownloadToAdopt::Box { name: b.name.clone(), provider: b.provider.clone(), version: b.version.clone() }));
    }
    new
}

/// Runs `op`, then adds to the runner's record what it downloaded of `images` and `boxes` (see
/// [`appeared`]). Best effort: a failure is logged, and only means that download isn't cleaned up
/// later.
pub async fn tracked<T>(dirs: impl Into<AppDirs>, images: &[String], boxes: &[String], op: impl Future<Output = T>) -> T {
    let dirs = dirs.into();
    let before = Before::take(!images.is_empty(), !boxes.is_empty()).await;
    let out = op.await;
    let new = appeared(&before, images, boxes).await;
    if !new.is_empty() {
        let added = match crate::runtime::paths::runner_home(&dirs) {
            Ok(home) => crate::runtime::isothrum::adopt_downloads(&home, new).await.map(drop),
            Err(e) => Err(e),
        };
        if let Err(e) = added {
            log::warn!("what the self-test downloaded couldn't be added to the runner's record: {e}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

    #[tokio::test]
    async fn only_what_appeared_during_the_self_test_is_adopted() {
        // The player had postgres:16 and bento/debian-12 already; the self-test pulled nginx and
        // generic/alpine319.
        let mut listed = 0;
        let _tools = Tools::new(move |_, args| match args {
            ["image", "ls", ..] | [.., "postgres:16"] => ok("sha256:postgres\n"),
            [.., "nginx:1.27"] => ok("sha256:nginx\n"),
            [.., "missing:1"] => fail("No such image"),
            ["box", "list"] => {
                listed += 1;
                if listed == 1 {
                    ok("bento/debian-12 (virtualbox, 1)\n")
                } else {
                    ok("bento/debian-12 (virtualbox, 1)\ngeneric/alpine319 (virtualbox, 4)\nother/box (virtualbox, 2)\n")
                }
            }
            _ => ok(""),
        });
        let before = Before::take(true, true).await;
        let images = ["nginx:1.27".to_string(), "postgres:16".into(), "missing:1".into()];
        let boxes = ["generic/alpine319".to_string(), "bento/debian-12".into()];
        let new = appeared(&before, &images, &boxes).await;
        // `other/box` appeared too, but the self-test doesn't use it: the player's.
        assert_eq!(
            new,
            [
                DownloadToAdopt::Image { reference: "nginx:1.27".into(), image_id: Some("sha256:nginx".into()) },
                DownloadToAdopt::Box { name: "generic/alpine319".into(), provider: "virtualbox".into(), version: "4".into() },
            ]
        );
    }

    #[tokio::test]
    async fn nothing_is_adopted_when_the_tools_couldnt_say_what_was_there() {
        let _tools = Tools::rules(&[("image ls", fail("Cannot connect to the Docker daemon")), ("box list", None), ("inspect", ok("sha256:x\n"))]);
        let before = Before::take(true, true).await;
        assert_eq!(appeared(&before, &["nginx:1.27".into()], &["a/b".into()]).await, []);
    }
}
