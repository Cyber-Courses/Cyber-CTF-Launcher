//! What Cyber CTF downloaded itself, recorded in `<app data>/storage.json`: the container images
//! (each by the reference it pulled and the image id it got) and the Vagrant boxes (name,
//! provider, version) that appeared on this machine during one of its own operations (a lab
//! start, a container attack box, a self-test). Storage lists and removes only these: an
//! image or a box the player already had, or pulled for their own projects, is never touched,
//! even when a lab uses the same one.
//!
//! The record's shape (`Owned`/`OwnedImage`/`OwnedBox`), its `vagrant box list` parser and its
//! loader are in [`record`]. The process-reading parts (what is on the
//! machine now, recording what an operation downloaded) stay here on the launcher's `platform::exec`
//! so the self-test, launch and storage-page tests keep mocking them through `exec::fake::Tools`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod record;

use record::parse_box_list;
pub use record::{Owned, OwnedBox, OwnedImage, load};

use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::exec::run_read;

/// The record's file.
pub fn path(dirs: &AppDirs) -> Result<PathBuf> {
    Ok(dirs.data()?.join("storage.json"))
}

/// Changes the record at `store` with `change`, holding an OS lock on `<store>.lock` meanwhile:
/// a deploy worker and the app can record at once.
pub fn update(store: &Path, change: impl FnOnce(&mut Owned)) -> Result<()> {
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(store.with_extension("lock"))?;
    lock.lock()?;
    let mut owned = load(store)?;
    change(&mut owned);
    let json = serde_json::to_vec_pretty(&owned).map_err(|e| Error::Invalid(e.to_string()))?;
    crate::platform::atomic_file::write(store, json)?;
    Ok(())
}

/// What was on this machine before an operation that may download: the image ids and the boxes,
/// each only when asked for and when Docker or Vagrant could say (`None`: nothing is claimed).
pub struct Before {
    images: Option<BTreeSet<String>>,
    boxes: Option<BTreeSet<OwnedBox>>,
}

impl Before {
    pub async fn take(images: bool, boxes: bool) -> Before {
        let (images, boxes) = tokio::join!(async { if images { image_ids().await } else { None } }, async { if boxes { box_list().await } else { None } });
        Before { images, boxes }
    }

    /// Whether images are compared (worth listing the ones an operation used).
    pub fn tracks_images(&self) -> bool {
        self.images.is_some()
    }
}

/// Every image id on this machine; `None` when Docker can't say.
pub async fn image_ids() -> Option<BTreeSet<String>> {
    let out = run_read("docker", &["image", "ls", "-q", "--no-trunc"], None).await.ok()?;
    Some(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// The image `reference` names now.
async fn image_id(reference: &str) -> Option<String> {
    let out = run_read("docker", &["image", "inspect", "--format", "{{.Id}}", reference], None).await.ok()?;
    Some(out.trim().to_string()).filter(|id| !id.is_empty())
}

/// Every box version on this machine; `None` when Vagrant can't say.
pub async fn box_list() -> Option<BTreeSet<OwnedBox>> {
    run_read("vagrant", &["box", "list"], None).await.ok().map(|out| parse_box_list(&out))
}

/// Records as the app's what appeared since `before` among what an operation used: the images of
/// `images` (references) and the box versions named in `boxes`. Best effort: a failure is logged,
/// and only means that download isn't cleaned up later.
pub async fn claim(store: &Path, before: &Before, images: &[String], boxes: &[String]) {
    let mut new_images = Vec::new();
    if let Some(had) = &before.images {
        for reference in images {
            if let Some(id) = image_id(reference).await.filter(|id| !had.contains(id)) {
                new_images.push(OwnedImage { reference: reference.clone(), id });
            }
        }
    }
    let new_boxes: Vec<OwnedBox> = match (&before.boxes, boxes.is_empty()) {
        (Some(had), false) => box_list().await.map(|now| now.difference(had).filter(|b| boxes.contains(&b.name)).cloned().collect()).unwrap_or_default(),
        _ => Vec::new(),
    };
    if new_images.is_empty() && new_boxes.is_empty() {
        return;
    }
    if let Err(e) = update(store, |owned| {
        owned.images.extend(new_images);
        owned.boxes.extend(new_boxes);
    }) {
        log::warn!("couldn't record the app's downloads in {}: {e}", store.display());
    }
}

/// Runs `op`, then records what it downloaded of `images` and `boxes` (see [`claim`]): for
/// operations whose downloads are named up front. The record is in `dirs`' data folder.
pub async fn tracked<T>(dirs: impl Into<AppDirs>, images: &[String], boxes: &[String], op: impl Future<Output = T>) -> T {
    let dirs = dirs.into();
    let before = Before::take(!images.is_empty(), !boxes.is_empty()).await;
    let out = op.await;
    if let Ok(store) = path(&dirs) {
        claim(&store, &before, images, boxes).await;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

    fn store() -> PathBuf {
        std::env::temp_dir().join(format!("cyberctf-owned-{}", rand::random::<u32>())).join("storage.json")
    }

    #[tokio::test]
    async fn only_what_appeared_during_the_operation_is_claimed() {
        let store = store();
        // The player had postgres:16 and bento/debian-12 already; the operation pulled nginx and
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
        claim(&store, &before, &images, &boxes).await;
        let owned = load(&store).unwrap();
        assert_eq!(owned.images.into_iter().collect::<Vec<_>>(), [OwnedImage { reference: "nginx:1.27".into(), id: "sha256:nginx".into() }]);
        // `other/box` appeared too, but this operation doesn't use it: the player's.
        assert_eq!(
            owned.boxes.into_iter().collect::<Vec<_>>(),
            [OwnedBox { name: "generic/alpine319".into(), provider: "virtualbox".into(), version: "4".into() }]
        );
        std::fs::remove_dir_all(store.parent().unwrap()).ok();
    }

    #[tokio::test]
    async fn nothing_is_claimed_when_the_tools_couldnt_say_what_was_there() {
        let store = store();
        let _tools = Tools::rules(&[("image ls", fail("Cannot connect to the Docker daemon")), ("box list", None), ("inspect", ok("sha256:x\n"))]);
        let before = Before::take(true, true).await;
        assert!(!before.tracks_images());
        claim(&store, &before, &["nginx:1.27".into()], &["a/b".into()]).await;
        assert!(!store.exists());
    }

    #[test]
    fn updates_add_to_the_record_and_a_damaged_one_is_left_alone() {
        let store = store();
        let image = |r: &str| OwnedImage { reference: r.into(), id: format!("sha256:{r}") };
        update(&store, |o| {
            o.images.insert(image("a"));
        })
        .unwrap();
        update(&store, |o| {
            o.images.insert(image("b"));
        })
        .unwrap();
        assert_eq!(load(&store).unwrap().images.len(), 2);
        // A damaged record is an error, never read as empty and written back: that would forget
        // every download it lists.
        std::fs::write(&store, "{not json").unwrap();
        assert!(load(&store).is_err());
        assert!(
            update(&store, |o| {
                o.images.insert(image("c"));
            })
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&store).unwrap(), "{not json", "the damaged record is not overwritten");
        std::fs::remove_dir_all(store.parent().unwrap()).ok();
    }
}
