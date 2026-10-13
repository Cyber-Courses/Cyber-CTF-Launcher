//! Files and folders only this user can open: the deploy jobs (they carry the launch token), the
//! deploy logs, the Terraform state (it holds the labs' sensitive outputs) and the cloud
//! credentials written for a run. On Unix files are created 0600 and folders made 0700; the
//! usual umask (022) would leave them readable by every local account. Elsewhere the per-user
//! data folder already keeps other accounts out, and these are plain writes.

use std::fs::File;
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
    let _ = std::fs::remove_file(file);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, FILE_MODE);
    options.open(file)?.write_all(contents.as_ref())
}

/// Opens `file` to write a fresh log, owner-only: emptied when it exists (an older, wider one
/// is narrowed), created otherwise.
pub fn create(file: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    open(file, &options)
}

/// Opens `file` to append to it, owner-only: created when missing, narrowed when wider.
pub fn append(file: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    open(file, &options)
}

fn open(file: &Path, options: &std::fs::OpenOptions) -> io::Result<File> {
    #[cfg(unix)]
    {
        let mut options = options.clone();
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, FILE_MODE);
        let f = options.open(file)?;
        // `mode` only applies to a file it creates.
        narrow(&f, FILE_MODE)?;
        Ok(f)
    }
    #[cfg(not(unix))]
    options.open(file)
}

/// Creates `dir` (with its missing parents, which get the usual mode) and makes it owner-only.
pub fn dir(dir: &Path) -> io::Result<()> {
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

/// Sets an open file's mode to `mode` when it is anything else.
#[cfg(unix)]
fn narrow(f: &File, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if f.metadata()?.permissions().mode() & 0o777 != mode {
        f.set_permissions(std::fs::Permissions::from_mode(mode))?;
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
        {
            use std::io::Write;
            let mut log = create(&d.join("x.log")).unwrap();
            log.write_all(b"one\n").unwrap();
            let mut more = append(&d.join("x.log")).unwrap();
            more.write_all(b"two\n").unwrap();
        }
        assert_eq!(std::fs::read_to_string(d.join("x.log")).unwrap(), "one\ntwo\n");
        // A fresh log empties the old one.
        drop(create(&d.join("x.log")).unwrap());
        assert_eq!(std::fs::read_to_string(d.join("x.log")).unwrap(), "");
        dir(&d.join("a").join("b")).unwrap();
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
        let log = d.join("deploy.log");
        std::fs::write(&log, "old").unwrap();
        wide(&log, 0o666);
        drop(create(&log).unwrap());
        assert_eq!(mode(&log), 0o600);
        wide(&log, 0o644);
        drop(append(&log).unwrap());
        assert_eq!(mode(&log), 0o600);
        drop(create(&d.join("new.log")).unwrap());
        assert_eq!(mode(&d.join("new.log")), 0o600);
        let sub = d.join("deploys");
        std::fs::create_dir(&sub).unwrap();
        wide(&sub, 0o755);
        dir(&sub).unwrap();
        assert_eq!(mode(&sub), 0o700);
        dir(&d.join("fresh")).unwrap();
        assert_eq!(mode(&d.join("fresh")), 0o700);
        let _ = std::fs::remove_dir_all(&d);
    }
}
