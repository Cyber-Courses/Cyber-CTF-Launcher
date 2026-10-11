//! Whole-file writes for the app's own JSON stores (server profiles, settings, records): written
//! to a temporary file next to the target, flushed, then renamed over it. A crash or a full disk
//! mid-write leaves the previous file intact instead of a truncated one that no longer parses.

use std::io::Write;
use std::path::Path;

/// Writes `contents` to `path` atomically (the parent folder is created when missing).
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    // Unique per write, so two writers never share a temporary file.
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}-{:08x}.tmp", std::process::id(), rand::random::<u32>()));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_ref())?;
        f.sync_all()
    })();
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_file_whole_and_leaves_no_temporary_file() {
        let dir = std::env::temp_dir().join(format!("cyberctf-atomic-{}", rand::random::<u32>()));
        let f = dir.join("sub").join("store.json");
        write(&f, "{\"a\":1}").unwrap();
        write(&f, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}");
        let names: Vec<_> = std::fs::read_dir(f.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["store.json"]);
        // A target that can't be replaced (a folder) fails and leaves nothing behind.
        let folder = dir.join("sub").join("folder.json");
        std::fs::create_dir_all(folder.join("x")).unwrap();
        assert!(write(&folder, "{}").is_err());
        assert_eq!(std::fs::read_dir(f.parent().unwrap()).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
