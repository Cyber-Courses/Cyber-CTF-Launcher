//! What Cyber CTF stores on this machine, for the Machine page: the container images and
//! Vagrant boxes it downloaded itself, with sizes, and a clean-up that removes only those. The
//! runner records what its runs fetch and removes only that (`Runner::downloads`); the app adds to
//! its record the attack box's images (see [`ATTACK_BOX_REPOSITORY`]). An image or box the player had before, or pulled for their own
//! projects, is never listed nor removed, even when a lab uses the same one. Images still used by
//! a container, or boxes used by a VM, are left alone (Docker and Vagrant refuse).

use std::collections::BTreeMap;
use std::path::Path;

use isothrum_runner::{Download, DownloadItem, DownloadToAdopt, DownloadsToRemove, RemovalOutcome};
use serde::Serialize;

use crate::error::Result;
use crate::platform::AppDirs;
use crate::runtime::isothrum as remote;

/// The attack box's own repository. Its images are the app's by name: earlier launchers pulled
/// them before downloads were recorded, and nobody else publishes it.
const ATTACK_BOX_REPOSITORY: &str = "cyberctf/attack-box";

/// One download the app keeps on this machine, and the room it takes.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StoredItem {
    /// How it is listed: an image's reference, a box as `name (provider, version)`.
    pub name: String,
    /// Its size on disk, in bytes.
    pub bytes: u64,
}

/// The downloads the app keeps on this machine, for the Maintenance panel.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    /// Container images the app downloaded, present locally.
    pub images: Vec<StoredItem>,
    /// Vagrant boxes the app downloaded, present locally.
    pub boxes: Vec<StoredItem>,
}

impl Storage {
    fn total(&self) -> u64 {
        self.images.iter().chain(&self.boxes).map(|i| i.bytes).sum()
    }
}

/// The attack-box images of `extra_images` (those the UI knows about) to adopt: only those of the
/// attack box's own repository, whatever image they name now.
fn attack_box_images(extra_images: &[String]) -> Vec<DownloadToAdopt> {
    extra_images
        .iter()
        .filter(|i| crate::platform::exec::valid_image(i) && crate::platform::exec::ImageRef::parse(i).repository == ATTACK_BOX_REPOSITORY)
        .map(|i| DownloadToAdopt::Image { reference: i.clone(), image_id: None })
        .collect()
}

/// How the runner's record lists on the Machine page: the downloads here (their size read), one
/// row per image (two references to one image are one download on disk) and per box version for
/// its provider (its architectures together), each kind by name.
fn storage_of(downloads: &[Download]) -> Storage {
    let mut images: BTreeMap<&str, (&str, u64)> = BTreeMap::new();
    let mut boxes: BTreeMap<String, u64> = BTreeMap::new();
    for d in downloads {
        let Some(bytes) = d.size_bytes else { continue };
        match &d.item {
            DownloadItem::Image { reference, image_id, .. } => {
                images.entry(image_id).or_insert((reference, bytes));
            }
            DownloadItem::Box { name, provider, version, .. } => *boxes.entry(format!("{name} ({provider}, {version})")).or_default() += bytes,
            _ => {}
        }
    }
    let mut images: Vec<StoredItem> = images.into_values().map(|(name, bytes)| StoredItem { name: name.to_owned(), bytes }).collect();
    images.sort_by(|a, b| a.name.cmp(&b.name));
    Storage { images, boxes: boxes.into_iter().map(|(name, bytes)| StoredItem { name, bytes }).collect() }
}

/// What the runner of `home` records, once the attack box's images of `extra_images` are in its
/// record. An adoption that fails is logged: it is tried again at the next listing.
async fn recorded(home: &Path, extra_images: &[String]) -> Result<Vec<Download>> {
    let attack_box = attack_box_images(extra_images);
    if !attack_box.is_empty()
        && let Err(e) = remote::adopt_downloads(home, attack_box).await
    {
        log::warn!("the attack box's images couldn't be added to the runner's record: {e}");
    }
    remote::downloads(home).await
}

/// `extra_images`: the attack-box images the UI knows about; only those of the attack box's own
/// repository count (see [`attack_box_images`]), any other is the player's unless recorded.
pub async fn machine_storage(dirs: AppDirs, extra_images: Vec<String>) -> Result<Storage> {
    let home = crate::runtime::paths::runner_home(&dirs)?;
    Ok(storage_of(&recorded(&home, &extra_images).await?))
}

/// Removes what `machine_storage` lists: every download the runner records, each image reference
/// it pulled (untagged, so an image the player tagged too stays), each box version (never
/// `--all`). Ones still in use stay. Returns the bytes freed.
pub async fn machine_storage_clean(dirs: AppDirs, extra_images: Vec<String>) -> Result<u64> {
    let home = crate::runtime::paths::runner_home(&dirs)?;
    let before = storage_of(&recorded(&home, &extra_images).await?);
    for removal in remote::remove_downloads(&home, DownloadsToRemove::All).await? {
        if let RemovalOutcome::Failed { reason } = &removal.outcome {
            log::warn!("Storage's clean-up kept {:?}: {reason}", removal.download.map(|d| d.item));
        }
    }
    let after = storage_of(&remote::downloads(&home).await?);
    Ok(before.total().saturating_sub(after.total()))
}

#[cfg(test)]
mod tests;
