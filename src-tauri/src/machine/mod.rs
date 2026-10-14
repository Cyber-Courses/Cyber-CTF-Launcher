//! This machine: what it has installed (system check), its setup self-tests, and the
//! workloads and storage the Machine page shows.

pub mod images;
pub mod selftest;
pub mod storage;
pub mod system;
pub mod workloads;

use std::path::PathBuf;

use crate::AppHandle;
use tauri::Manager;

use crate::error::{Error, Result};

/// Where installed labs live: `<app data>/labs/<lab id>/`.
fn labs_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("labs"))
}
