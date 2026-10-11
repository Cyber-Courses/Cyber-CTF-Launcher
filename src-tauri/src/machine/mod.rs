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

/// Every installed lab: its id and its folder.
fn installed_labs(app: &AppHandle) -> Vec<(String, PathBuf)> {
    let Ok(dir) = labs_dir(app) else { return Vec::new() };
    labs_in(&dir)
}

/// The lab folders in `dir`, by id.
fn labs_in(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries.flatten().filter(|e| e.path().is_dir()).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn installed_labs_are_the_folders_of_the_labs_dir() {
        let dir = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
        std::fs::create_dir_all(dir.join("sqli")).unwrap();
        std::fs::write(dir.join("stray.txt"), "x").unwrap();
        let labs = super::labs_in(&dir);
        assert_eq!(labs, [("sqli".to_string(), dir.join("sqli"))]);
        assert!(super::labs_in(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
