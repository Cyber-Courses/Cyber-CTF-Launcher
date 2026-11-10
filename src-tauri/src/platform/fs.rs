//! Removing a file or folder that may already be gone: one already gone is no failure, any
//! other is the caller's to log or return (left behind, it is read again as if it still held).

use std::io;
use std::path::Path;

/// Removes the file at `path`; one already gone is no failure.
pub fn remove_if_present(path: &Path) -> io::Result<()> {
    gone_is_fine(std::fs::remove_file(path))
}

/// Removes the folder at `path` and everything in it; one already gone is no failure.
pub fn remove_dir_if_present(path: &Path) -> io::Result<()> {
    gone_is_fine(std::fs::remove_dir_all(path))
}

fn gone_is_fine(removed: io::Result<()>) -> io::Result<()> {
    match removed {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_what_is_there_and_takes_what_is_gone_as_removed() {
        let dir = std::env::temp_dir().join(format!("cyberctf-fs-{}", rand::random::<u32>()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("file"), "x").unwrap();
        remove_if_present(&dir.join("file")).unwrap();
        assert!(!dir.join("file").exists());
        remove_if_present(&dir.join("file")).unwrap();
        remove_dir_if_present(&dir.join("sub")).unwrap();
        assert!(!dir.join("sub").exists());
        remove_dir_if_present(&dir.join("sub")).unwrap();
        // Anything else is the caller's: a folder isn't a file.
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(remove_if_present(&dir.join("sub")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
