//! The Terraform provider-plugin cache on disk: its folder name, and clearing it from the Storage
//! page. The Isothrum runner runs Terraform now and keeps its own cache; this is the launcher's
//! older shared cache, still shown and cleanable on the Storage page.

use std::path::Path;

/// The shared plugin cache folder (under the app data dir): Terraform provider plugins, linked
/// into each run so they download once.
pub const PLUGIN_CACHE: &str = "terraform-plugins";

/// Empties the plugin cache, unless a run holds it (its `.lock`). Returns whether it ran.
pub fn clear_plugin_cache(dir: &Path) -> bool {
    let Ok(lock) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock")) else { return false };
    if lock.try_lock().is_err() {
        return false;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_name() != ".lock") {
        let _ = if entry.path().is_dir() { std::fs::remove_dir_all(entry.path()) } else { std::fs::remove_file(entry.path()) };
    }
    true
}
