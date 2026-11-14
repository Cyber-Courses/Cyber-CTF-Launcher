//! Files and folders only this user can open: the deploy jobs (they carry the launch token), the
//! deploy logs, the Terraform state (it holds the labs' sensitive outputs) and the cloud
//! credentials written for a run. On Unix files are created 0600 and folders made 0700; the
//! usual umask (022) would leave them readable by every local account. Elsewhere the per-user
//! data folder already keeps other accounts out, and these are plain writes.

use std::io;
use std::path::Path;

/// Owner read and write.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;
/// Owner read, write and search.
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;

/// Writes `contents` to `file`, owner-only from creation (never wider, even for a moment),
/// replacing any previous one. The parent folder is created when missing.
pub fn write(file: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    // A previous file keeps its mode when written over: start from none.
    super::fs::remove_if_present(file)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, FILE_MODE);
    options.open(file)?.write_all(contents.as_ref())
}

/// Creates `dir` (with its missing parents, which get the usual mode) and makes it owner-only.
pub fn create_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(dir)?.permissions().mode() & 0o777 != DIR_MODE {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(DIR_MODE))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A file's or folder's permission bits (Unix).
    #[cfg(unix)]
    pub(crate) fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Asserts the file at `path`, and its folder, are this user's only (on Unix; elsewhere the
    /// profile's ACL is relied on).
    pub(crate) fn assert_private_with_folder(path: &Path) {
        #[cfg(unix)]
        assert_eq!((mode(path), mode(path.parent().unwrap())), (0o600, 0o700), "{}", path.display());
        #[cfg(not(unix))]
        assert!(path.is_file(), "{}", path.display());
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cyberctf-private-{tag}-{}-{}", std::process::id(), rand::random::<u32>()))
    }

    #[test]
    fn files_are_written_whole_and_replaced() {
        let d = temp("write");
        let f = d.join("sub").join("job.json");
        write(&f, "first").unwrap();
        write(&f, "2nd").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "2nd");
        create_dir(&d.join("a").join("b")).unwrap();
        assert!(d.join("a").join("b").is_dir());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn files_and_folders_are_owner_only_even_over_wider_ones() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("modes");
        std::fs::create_dir_all(&d).unwrap();
        let wide = |p: &Path, m: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
        let f = d.join("job.json");
        std::fs::write(&f, "old").unwrap();
        wide(&f, 0o644);
        write(&f, "new").unwrap();
        assert_eq!(mode(&f), 0o600);
        let sub = d.join("deploys");
        std::fs::create_dir(&sub).unwrap();
        wide(&sub, 0o755);
        create_dir(&sub).unwrap();
        assert_eq!(mode(&sub), 0o700);
        create_dir(&d.join("fresh")).unwrap();
        assert_eq!(mode(&d.join("fresh")), 0o700);
        let _ = std::fs::remove_dir_all(&d);
    }
}
