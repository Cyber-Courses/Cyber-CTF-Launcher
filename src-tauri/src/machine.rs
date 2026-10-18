//! This machine: what it has installed (system check), its setup self-tests, and the
//! workloads and storage the Machine page shows.

pub mod images;
pub mod selftest;
pub mod storage;
pub mod system;
pub mod tools;
pub mod workloads;

use std::path::PathBuf;

use crate::error::Result;
use crate::platform::AppDirs;

/// Where installed labs live: `<app data>/labs/<lab id>/`.
fn labs_dir(dirs: &AppDirs) -> Result<PathBuf> {
    Ok(dirs.data()?.join("labs"))
}
