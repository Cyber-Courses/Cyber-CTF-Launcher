//! What Cyber CTF downloaded itself, recorded in `<app data>/storage.json`: the container images
//! (each by the reference it pulled and the image id it got) and the Vagrant boxes (name,
//! provider, version) that appeared on this machine during one of its own operations (a lab
//! start, the attack box or attack VM, a self-test). Storage lists and removes only these: an
//! image or a box the player already had, or pulled for their own projects, is never touched,
//! even when a lab uses the same one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::AppHandle;
use crate::error::{Error, Result};
use crate::exec::run_read;

/// An image the app pulled: the reference it asked for, and the image that reference named then.
/// The reference is untagged at clean-up only while it still names that image (one the player
/// pulled again since, or retagged, is theirs).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OwnedImage {
    pub reference: String,
    pub id: String,
}

/// A box version the app downloaded, for one provider.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OwnedBox {
    pub name: String,
    pub provider: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Owned {
    #[serde(default)]
    pub images: BTreeSet<OwnedImage>,
    #[serde(default)]
    pub boxes: BTreeSet<OwnedBox>,
}

/// The record's file.
pub fn path(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("storage.json"))
}

/// The record at `store`; empty when there is none yet (or it can't be read: nothing is then
/// the app's, which only means less is cleaned up).
pub fn load(store: &Path) -> Owned {
    std::fs::read(store).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Changes the record at `store` with `change`, holding an OS lock on `<store>.lock` meanwhile:
/// a deploy worker and the app can record at once.
pub fn update(store: &Path, change: impl FnOnce(&mut Owned)) -> Result<()> {
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(store.with_extension("lock"))?;
    lock.lock()?;
    let mut owned = load(store);
    change(&mut owned);
    let json = serde_json::to_vec_pretty(&owned).map_err(|e| Error::Invalid(e.to_string()))?;
    crate::atomic_file::write(store, json)?;
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
        Before { images: if images { image_ids().await } else { None }, boxes: if boxes { box_list().await } else { None } }
    }

    /// Whether images are compared (worth listing the ones an operation used).
    pub fn tracks_images(&self) -> bool {
        self.images.is_some()
    }
}

/// Every image id on this machine.
async fn image_ids() -> Option<BTreeSet<String>> {
    let out = run_read("docker", &["image", "ls", "-q", "--no-trunc"], None).await.ok()?;
    Some(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// The image `reference` names now.
async fn image_id(reference: &str) -> Option<String> {
    let out = run_read("docker", &["image", "inspect", "--format", "{{.Id}}", reference], None).await.ok()?;
    Some(out.trim().to_string()).filter(|id| !id.is_empty())
}

/// Every box version on this machine.
async fn box_list() -> Option<BTreeSet<OwnedBox>> {
    run_read("vagrant", &["box", "list"], None).await.ok().map(|out| parse_box_list(&out))
}

/// `vagrant box list` lines: `bento/debian-12   (virtualbox, 202510.26.0, (amd64))`.
pub fn parse_box_list(out: &str) -> BTreeSet<OwnedBox> {
    out.lines()
        .filter_map(|line| {
            let (name, rest) = line.trim().split_once(char::is_whitespace)?;
            let inner = rest.trim().strip_prefix('(')?;
            let mut fields = inner.split(',').map(|f| f.trim().trim_end_matches(')'));
            let provider = fields.next().filter(|p| !p.is_empty())?;
            let version = fields.next().filter(|v| !v.is_empty())?;
            Some(OwnedBox { name: name.to_string(), provider: provider.to_string(), version: version.to_string() })
        })
        .collect()
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
/// operations whose downloads are named up front.
pub async fn tracked<T>(app: &AppHandle, images: &[String], boxes: &[String], op: impl Future<Output = T>) -> T {
    let before = Before::take(!images.is_empty(), !boxes.is_empty()).await;
    let out = op.await;
    if let Ok(store) = path(app) {
        claim(&store, &before, images, boxes).await;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::fake::{Tools, fail, ok};

    fn store() -> PathBuf {
        std::env::temp_dir().join(format!("cyberctf-owned-{}", rand::random::<u32>())).join("storage.json")
    }

    #[test]
    fn box_listings_read_as_name_provider_and_version() {
        let out = "bento/debian-12   (virtualbox, 202510.26.0, (amd64))\ngeneric/alpine319 (libvirt, 4.3.12)\nThere are no installed boxes!\nodd (\n";
        let boxes: Vec<(String, String, String)> = parse_box_list(out).into_iter().map(|b| (b.name, b.provider, b.version)).collect();
        assert_eq!(
            boxes,
            [("bento/debian-12".into(), "virtualbox".into(), "202510.26.0".into()), ("generic/alpine319".into(), "libvirt".into(), "4.3.12".into())]
        );
    }

    #[tokio::test]
    async fn only_what_appeared_during_the_operation_is_claimed() {
        let store = store();
        // The player had postgres:16 and bento/debian-12 already; the operation pulled nginx and
        // generic/alpine319.
        let mut listed = 0;
        let _tools = Tools::new(move |_, args| match args {
            ["image", "ls", ..] => ok("sha256:postgres\n"),
            [.., "nginx:1.27"] => ok("sha256:nginx\n"),
            [.., "postgres:16"] => ok("sha256:postgres\n"),
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
        let owned = load(&store);
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
    fn updates_add_to_the_record_and_a_damaged_one_starts_over() {
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
        assert_eq!(load(&store).images.len(), 2);
        std::fs::write(&store, "{not json").unwrap();
        assert_eq!(load(&store), Owned::default());
        std::fs::remove_dir_all(store.parent().unwrap()).ok();
    }
}
