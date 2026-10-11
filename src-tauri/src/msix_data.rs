//! The Store (MSIX) build used to run with file-system write virtualization on, so what it wrote
//! under AppData went to the package's private copy (`Packages\<family>\LocalCache\...`), where
//! docker, vagrant, terraform and ssh can't see it. Virtualization is now off (AppxManifest.xml);
//! this brings the files saved before over to the real folders, once, never overwriting one.

use std::path::Path;

/// Copies every file under `src` that `dst` doesn't have yet (recursively). Errors on one file
/// don't stop the others.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn merge_missing(src: &Path, dst: &Path) {
    let Ok(entries) = std::fs::read_dir(src) else { return };
    for entry in entries.flatten() {
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            merge_missing(&from, &to);
        } else if kind.is_file() && !to.exists() && std::fs::create_dir_all(dst).is_ok() {
            let _ = std::fs::copy(&from, &to);
        }
    }
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
            merge_missing(&cache.join("Local").join(id), &dst);
        }
        if let Ok(dst) = paths.app_data_dir() {
            merge_missing(&cache.join("Roaming").join(id), &dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::merge_missing;

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

        merge_missing(&src, &dst);

        assert_eq!(std::fs::read_to_string(dst.join("server.json")).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(dst.join("deploys").join("lab.log")).unwrap(), "log");
        assert_eq!(std::fs::read_to_string(dst.join("shared-folder.json")).unwrap(), "new", "never overwritten");
        merge_missing(&root.join("missing"), &dst);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
