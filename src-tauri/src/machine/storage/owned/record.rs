//! The record of what a program downloaded for its own operations, kept as `storage.json` in its
//! data folder: the container images (each by the reference it pulled and the id that reference
//! named) and the Vagrant boxes (name, provider, version) that appeared during one of its
//! operations. Its cleanup lists and removes only these: an image or a box the user already had,
//! or pulled for their own projects, is never touched, even when one of those operations uses the
//! same one. This module is the record's shape, its loader and the `vagrant box list` parser;
//! the program that keeps the record writes it.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// An image the program pulled: the reference it asked for, and the image that reference named
/// then.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OwnedImage {
    /// The image reference pulled, e.g. `nginx:1.27`.
    pub reference: String,
    /// The image id that reference named when it was pulled.
    pub id: String,
}

/// A box version the program downloaded, for one provider.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OwnedBox {
    /// The box name, e.g. `generic/debian12`.
    pub name: String,
    /// The provider the box version is for, e.g. `virtualbox`.
    pub provider: String,
    /// The version downloaded.
    pub version: String,
}

/// The record of everything the program downloaded.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct Owned {
    /// The images pulled.
    #[serde(default)]
    pub images: BTreeSet<OwnedImage>,
    /// The box versions downloaded.
    #[serde(default)]
    pub boxes: BTreeSet<OwnedBox>,
}

/// The record at `store`; empty when there is none yet.
///
/// # Errors
///
/// The file exists but can't be read, or isn't a record (`InvalidData`). A caller that changes
/// the record and writes it back must stop there: writing an empty record over one it couldn't
/// read would forget everything the program downloaded.
pub fn load(store: &Path) -> std::io::Result<Owned> {
    match std::fs::read(store) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Owned::default()),
        Err(e) => Err(e),
    }
}

/// A box as one line of `vagrant box list` names it: the box version for its provider, and its
/// architecture when the listing names one (`amd64`, `arm64`; older Vagrants don't).
#[derive(Debug, PartialEq, Eq)]
pub struct ListedBox {
    /// The box version, for its provider.
    pub owned: OwnedBox,
    /// Its architecture, when the listing names one.
    pub arch: Option<String>,
}

/// `vagrant box list` lines: `generic/debian12   (virtualbox, 202510.26.0, (amd64))`. A line of
/// another shape (`There are no installed boxes!`) is skipped. The one reader of that listing.
#[must_use]
pub fn listed_boxes(out: &str) -> Vec<ListedBox> {
    out.lines()
        .filter_map(|line| {
            let (name, rest) = line.trim().split_once(char::is_whitespace)?;
            let inner = rest.trim().strip_prefix('(')?;
            let mut fields = inner.split(',').map(|f| f.trim().trim_end_matches(')'));
            let provider = fields.next().filter(|p| !p.is_empty())?;
            let version = fields.next().filter(|v| !v.is_empty())?;
            let arch = fields.next().map(|a| a.trim_start_matches('(')).filter(|a| !a.is_empty()).map(str::to_owned);
            Some(ListedBox { owned: OwnedBox { name: name.to_owned(), provider: provider.to_owned(), version: version.to_owned() }, arch })
        })
        .collect()
}

/// The box versions in a `vagrant box list` (see [`listed_boxes`]), whatever their architecture.
#[must_use]
pub fn parse_box_list(out: &str) -> BTreeSet<OwnedBox> {
    listed_boxes(out).into_iter().map(|b| b.owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_box_keeps_its_architecture() {
        let out = "generic/debian12   (virtualbox, 202510.26.0, (amd64))\r\ngeneric/alpine319\t(libvirt, 4.3.12)\n";
        let arch: Vec<(String, Option<String>)> = listed_boxes(out).into_iter().map(|b| (b.owned.name, b.arch)).collect();
        assert_eq!(arch, [("generic/debian12".to_owned(), Some("amd64".to_owned())), ("generic/alpine319".to_owned(), None)]);
    }

    #[test]
    fn box_listings_read_as_name_provider_and_version() {
        let out = "generic/debian12   (virtualbox, 202510.26.0, (amd64))\ngeneric/alpine319 (libvirt, 4.3.12)\nThere are no installed boxes!\nodd (\n";
        let boxes: Vec<(String, String, String)> = parse_box_list(out).into_iter().map(|b| (b.name, b.provider, b.version)).collect();
        assert_eq!(
            boxes,
            [
                ("generic/alpine319".to_owned(), "libvirt".to_owned(), "4.3.12".to_owned()),
                ("generic/debian12".to_owned(), "virtualbox".to_owned(), "202510.26.0".to_owned()),
            ]
        );
    }

    #[test]
    fn a_record_loads_a_missing_one_is_empty_and_a_damaged_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("isothrum-owned-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = dir.join("storage.json");
        assert_eq!(load(&store).unwrap(), Owned::default());
        std::fs::write(&store, r#"{"images":[{"reference":"nginx:1.27","id":"sha256:nginx"}]}"#).unwrap();
        let owned = load(&store).unwrap();
        assert_eq!(owned.images.len(), 1);
        assert!(owned.boxes.is_empty());
        std::fs::write(&store, "{not json").unwrap();
        assert_eq!(load(&store).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        assert!(load(&dir).is_err(), "a folder where the file should be is an error, not an empty record");
        std::fs::remove_dir_all(&dir).ok();
    }
}
