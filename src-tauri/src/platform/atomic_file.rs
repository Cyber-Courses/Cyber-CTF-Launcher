//! Whole-file writes for the app's own JSON stores (server profiles, settings, records): written
//! to a temporary file next to the target, flushed, then renamed over it. A crash or a full disk
//! mid-write leaves the previous file intact instead of a truncated one that no longer parses.

use std::io::Write;
use std::path::Path;

/// Writes `contents` to `path` atomically (the parent folder is created when missing).
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    write_as(path, contents.as_ref(), false)
}

/// [`write`] for a file only this user may read (0600 on Unix, from the temporary file on), such
/// as a session's tokens.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn write_private(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    write_as(path, contents.as_ref(), true)
}

#[cfg_attr(not(unix), allow(unused_variables))]
fn write_as(path: &Path, contents: &[u8], private: bool) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    // Unique per write, so two writers never share a temporary file.
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}-{:08x}.tmp", std::process::id(), rand::random::<u32>()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        }
        let mut f = options.open(&tmp)?;
        f.write_all(contents)?;
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

    #[test]
    fn a_private_file_is_owner_only_even_over_a_wider_one() {
        let dir = std::env::temp_dir().join(format!("cyberctf-atomic-private-{}", rand::random::<u32>()));
        let f = dir.join("session.json");
        write(&f, "old").unwrap();
        write_private(&f, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}");
        #[cfg(unix)]
        {
            assert_eq!(crate::platform::private_fs::tests::mode(&f), 0o600);
            write(&dir.join("plain.json"), "{}").unwrap();
            assert_ne!(crate::platform::private_fs::tests::mode(&dir.join("plain.json")), 0o600, "a plain store keeps the usual mode");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fresh(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-atomic-{tag}-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn concurrent_writers_leave_one_whole_file_and_no_temporaries() {
        let dir = fresh("race");
        let f = dir.join("store.json");
        let bodies: Vec<String> = (0..8).map(|i| format!("{{\"writer\":{i},\"pad\":\"{}\"}}", "x".repeat(50_000))).collect();
        let handles: Vec<_> = bodies
            .iter()
            .cloned()
            .map(|body| {
                let f = f.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        write(&f, &body).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let got = std::fs::read_to_string(&f).unwrap();
        assert!(bodies.contains(&got), "the file is one writer's whole body, never a mix");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, ["store.json"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_parent_that_is_a_file_fails_and_leaves_nothing() {
        let dir = fresh("parent-file");
        std::fs::write(dir.join("notadir"), "x").unwrap();
        let err = write(&dir.join("notadir").join("store.json"), "{}").unwrap_err();
        assert_ne!(err.kind(), std::io::ErrorKind::Other, "{err}");
        assert_eq!(std::fs::read_to_string(dir.join("notadir")).unwrap(), "x");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_and_unicode_named_files_are_written_whole() {
        let dir = fresh("names");
        let f = dir.join("état · ⚙.json");
        write(&f, "").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"");
        let big = "y".repeat(4 * 1024 * 1024);
        write(&f, &big).unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().len(), big.len() as u64);
        write(&f, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}", "shrinks, never keeps a tail of the old file");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["état · ⚙.json"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn writing_through_a_symlink_replaces_the_link_not_its_target() {
        // The rename lands a new file where the link was; the target keeps its old contents.
        // That is what the app's stores want (its own file under its own folder), stated here so
        // a change to in-place writing is a deliberate one.
        let dir = fresh("symlink");
        let target = dir.join("real.json");
        std::fs::write(&target, "old").unwrap();
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write(&link, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "new");
        assert!(!std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_temporary_left_by_a_crash_does_not_block_the_next_write() {
        // A write cut short (power loss after the create) leaves `.store.json.<pid>-<x>.tmp`;
        // the next write uses a name of its own and lands. The stray file is nobody's to remove
        // here, so it stays (a store folder may accumulate them; see the report).
        let dir = fresh("stray");
        let f = dir.join("store.json");
        let stray = dir.join(".store.json.1-00000001.tmp");
        std::fs::write(&stray, "half").unwrap();
        write(&f, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}");
        assert!(stray.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
