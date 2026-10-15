//! The Store (MSIX) build used to run with file-system write virtualization on, so what it wrote
//! under AppData went to the package's private copy (`Packages\<family>\LocalCache\...`), where
//! docker, vagrant, terraform and ssh can't see it. Virtualization is now off (packaging/msix/AppxManifest.xml);
//! this brings the files saved before over to the real folders, once, never overwriting one.

use std::path::Path;

/// Left in a destination folder once the old files were brought over: the merge then never runs
/// again. It ran on every start before, and brought back what had since been removed on
/// purpose (a destroyed lab's terraform.tfstate, removed labs, server.json, deploy job files).
const MIGRATED: &str = ".msix-data-migrated";

/// [`merge_missing`] from `src` to `dst`, the first time only: skipped once `dst` has the
/// marker, which is written after a merge that copied everything. A merge that failed on a file
/// runs again next time (copying only what is still missing). No source, nothing to do.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn migrate_once(src: &Path, dst: &Path) {
    let marker = dst.join(MIGRATED);
    if marker.exists() || !src.is_dir() {
        return;
    }
    if merge_missing(src, dst) {
        let _ = std::fs::create_dir_all(dst).and_then(|_| std::fs::write(&marker, b""));
    }
}

/// Copies every file under `src` that `dst` doesn't have yet (recursively). Errors on one file
/// don't stop the others. Whether every file was there or copied.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn merge_missing(src: &Path, dst: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(src) else { return false };
    let mut all = true;
    for entry in entries {
        let Ok(entry) = entry else {
            all = false;
            continue;
        };
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let Ok(kind) = entry.file_type() else {
            all = false;
            continue;
        };
        if kind.is_dir() {
            all &= merge_missing(&from, &to);
        } else if kind.is_file() && !to.exists() {
            all &= std::fs::create_dir_all(dst).is_ok() && std::fs::copy(&from, &to).is_ok();
        }
    }
    all
}

/// Windows: brings over what an earlier, virtualized Store build saved. No-op when there is none.
#[cfg(windows)]
pub(crate) fn migrate<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from) else { return };
    let Ok(packages) = std::fs::read_dir(local.join("Packages")) else { return };
    let id = &app.config().identifier;
    let paths = app.path();
    for family in packages.flatten().filter(|p| p.file_name().to_string_lossy().starts_with("F4K.CyberCTF_")) {
        let cache = family.path().join("LocalCache");
        if let Ok(dst) = paths.app_local_data_dir() {
            migrate_once(&cache.join("Local").join(id), &dst);
        }
        if let Ok(dst) = paths.app_data_dir() {
            migrate_once(&cache.join("Roaming").join(id), &dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MIGRATED, merge_missing, migrate_once};

    #[test]
    fn copies_what_is_missing_and_keeps_what_is_there() {
        let root = std::env::temp_dir().join(format!("cyberctf-msix-{}", std::process::id()));
        let (src, dst) = (root.join("src"), root.join("dst"));
        std::fs::create_dir_all(src.join("deploys")).unwrap();
        std::fs::write(src.join("server.json"), "old").unwrap();
        std::fs::write(src.join("deploys").join("lab.log"), "log").unwrap();
        std::fs::write(src.join("shared-folder.json"), "old").unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("shared-folder.json"), "new").unwrap();

        assert!(merge_missing(&src, &dst));

        assert_eq!(std::fs::read_to_string(dst.join("server.json")).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(dst.join("deploys").join("lab.log")).unwrap(), "log");
        assert_eq!(std::fs::read_to_string(dst.join("shared-folder.json")).unwrap(), "new", "never overwritten");
        assert!(!merge_missing(&root.join("missing"), &dst));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_old_files_are_brought_over_once_only() {
        // Regression: with no record of the merge it ran on every start, so after a destroy the
        // old terraform.tfstate, the removed labs and server.json came back each time.
        let root = std::env::temp_dir().join(format!("cyberctf-msix-once-{}", rand::random::<u32>()));
        let (src, dst) = (root.join("src"), root.join("dst"));
        std::fs::create_dir_all(src.join("labs").join("cloud")).unwrap();
        std::fs::write(src.join("labs").join("cloud").join("terraform.tfstate"), "old state").unwrap();
        std::fs::write(src.join("server.json"), "old").unwrap();

        migrate_once(&src, &dst);
        assert_eq!(std::fs::read_to_string(dst.join("server.json")).unwrap(), "old");
        assert!(dst.join(MIGRATED).exists(), "the merge is recorded");

        // The lab destroyed and the server forgotten: a later start leaves them gone.
        std::fs::remove_dir_all(dst.join("labs")).unwrap();
        std::fs::remove_file(dst.join("server.json")).unwrap();
        migrate_once(&src, &dst);
        assert!(!dst.join("labs").exists() && !dst.join("server.json").exists(), "nothing came back");

        // No old files at all: nothing to do, and nothing recorded.
        let fresh = root.join("fresh");
        migrate_once(&root.join("missing"), &fresh);
        assert!(!fresh.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_merge_that_missed_a_file_runs_again() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("cyberctf-msix-retry-{}", rand::random::<u32>()));
        let (src, dst) = (root.join("src"), root.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("server.json"), "old").unwrap();
        let unreadable = src.join("keys.json");
        std::fs::write(&unreadable, "k").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads it anyway (some CI containers): then there's no failure to see.
        if std::fs::read(&unreadable).is_err() {
            migrate_once(&src, &dst);
            assert!(dst.join("server.json").exists());
            assert!(!dst.join(MIGRATED).exists(), "not recorded while a file is missing");
            std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
            migrate_once(&src, &dst);
            assert_eq!(std::fs::read_to_string(dst.join("keys.json")).unwrap(), "k");
            assert!(dst.join(MIGRATED).exists());
        }
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }
}
