//! The Terraform provider-plugin cache on disk: its folder name, and clearing it from the Storage
//! page. The Isothrum runner runs Terraform now and keeps its own cache; this is the launcher's
//! older shared cache, still shown and cleanable on the Storage page.

use std::path::Path;

use crate::platform::fs::{remove_dir_if_present, remove_if_present};

/// The shared plugin cache folder (under the app data dir): Terraform provider plugins, linked
/// into each run so they download once.
pub const PLUGIN_CACHE: &str = "terraform-plugins";

/// Empties the plugin cache, unless a run holds it (its `.lock`). What stays (the cache held, a
/// plugin that won't go) is logged, as Storage's other clean-ups are: the clean-up goes on, and
/// frees less.
pub fn clear_plugin_cache(dir: &Path) {
    let lock = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock")) {
        Ok(lock) => lock,
        Err(e) => {
            log::warn!("Storage's clean-up kept the Terraform plugins: their lock {} can't be opened: {e}", dir.join(".lock").display());
            return;
        }
    };
    if lock.try_lock().is_err() {
        log::info!("Storage's clean-up kept the Terraform plugins: a run uses them");
        return;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_name() != ".lock") {
        let path = entry.path();
        let removed = if path.is_dir() { remove_dir_if_present(&path) } else { remove_if_present(&path) };
        if let Err(e) = removed {
            log::warn!("Storage's clean-up kept {}: {e}", path.display());
        }
    }
}
