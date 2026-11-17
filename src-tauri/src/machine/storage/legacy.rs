//! What earlier launchers recorded of their own downloads, taken into the runner's record once:
//! `storage.json` in the data folder (1.9 and earlier) and the launcher store's own record (the
//! development builds after it). Each image and box goes to the runner as it was recorded
//! (`Runner::adopt_downloads`: one no longer on this machine as it was fetched isn't adopted),
//! then the launcher's record is removed: its removal is the mark that it was taken. One the
//! runner couldn't take (the engine or Vagrant couldn't say what they hold, the service down) is
//! logged and left where it is for the next listing: nothing a player downloaded is forgotten.

use std::path::Path;

use isothrum_runner::DownloadToAdopt;

use super::owned::Owned;
use crate::error::Result;
use crate::platform::AppDirs;
use crate::runtime::isothrum as remote;

/// The file 1.9 and earlier kept the record in, under the data folder, and its lock beside it.
const FILE: &str = "storage.json";
const LOCK: &str = "storage.json.lock";

/// Takes what earlier launchers recorded under `dirs`' data folder into the runner of `home`.
pub(super) async fn take_in(dirs: &AppDirs, home: &Path) {
    let Ok(data) = dirs.data() else { return };
    if let Err(e) = take_in_from(data, home).await {
        log::warn!("the downloads earlier launchers recorded couldn't be added to the runner's record (tried again at the next listing): {e}");
    }
}

/// [`take_in`] from data folder `data`.
async fn take_in_from(data: &Path, home: &Path) -> Result<()> {
    let file = data.join(FILE);
    let from_file: Owned = match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| crate::error::Error::Invalid(format!("{} can't be read: {e}", file.display())))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Owned::default(),
        Err(e) => return Err(e.into()),
    };
    let in_store = if data.join(crate::store::FILE).is_file() { crate::store::open_in(data)?.read(crate::store::storage::load)? } else { Owned::default() };
    let adopted: Vec<DownloadToAdopt> = [&from_file, &in_store].into_iter().flat_map(to_adopt).collect();
    if !adopted.is_empty() {
        remote::adopt_downloads(home, adopted).await?;
    }
    if !in_store.images.is_empty() || !in_store.boxes.is_empty() {
        crate::store::open_in(data)?.write(|tx| crate::store::storage::replace(tx, &Owned::default()))?;
    }
    for done in [FILE, LOCK] {
        match std::fs::remove_file(data.join(done)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

/// `owned`'s images and boxes, as the runner adopts them: an image while its reference stands for
/// the id recorded.
fn to_adopt(owned: &Owned) -> impl Iterator<Item = DownloadToAdopt> + '_ {
    let images = owned.images.iter().map(|i| DownloadToAdopt::Image { reference: i.reference.clone(), image_id: Some(i.id.clone()) });
    let boxes = owned.boxes.iter().map(|b| DownloadToAdopt::Box { name: b.name.clone(), provider: b.provider.clone(), version: b.version.clone() });
    images.chain(boxes)
}
