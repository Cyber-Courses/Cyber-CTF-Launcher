//! The record of what a program downloaded for its own operations: the container images (each by the reference it pulled and the id that reference
//! named) and the Vagrant boxes (name, provider, version) that appeared during one of its
//! operations. Its cleanup lists and removes only these: an image or a box the user already had,
//! or pulled for their own projects, is never touched, even when one of those operations uses the
//! same one. Earlier launchers kept it (`storage.json`, then the store): the runner adopts it once
//! (`legacy`), and this is its shape.

use std::collections::BTreeSet;

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
