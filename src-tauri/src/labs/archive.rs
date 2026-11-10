//! A lab's GitHub archive unpacked into its folder: the top `<repo>-<sha>/` folder stripped,
//! nothing written outside the destination (no absolute path, no `..`, no link climbing out),
//! a size cap, and the names Windows can't hold refused plainly.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Extracts a GitHub tarball (single top-level `<repo>-<sha>/` folder) into
/// `dest`, rejecting absolute paths, `..` and links.
pub(super) fn extract(tarball: &[u8], dest: &Path) -> Result<()> {
    extract_within(tarball, dest, crate::platform::http::MAX_LAB_UNPACKED)
}

/// [`extract`], refused before the files would add up to more than `max` bytes.
fn extract_within(tarball: &[u8], dest: &Path, max: u64) -> Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    let mut unpacked: u64 = 0;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        let is_symlink = kind.is_symlink();
        if !(kind.is_file() || kind.is_dir() || is_symlink) {
            // Hard links, fifos and devices are not part of a lab's source: left out.
            continue;
        }
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > max {
            return Err(Error::Invalid(format!("the lab archive unpacks to more than {} MB; refused", max / (1024 * 1024))));
        }
        let path = entry.path()?.into_owned();
        // Past a leading `./` (GNU tar's `-C dir .` style), then the top folder GitHub names
        // `<repo>-<sha>`.
        let relative: PathBuf = path.components().skip_while(|c| matches!(c, std::path::Component::CurDir)).skip(1).collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        if relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(Error::Invalid(format!("unsafe path in lab archive: {}", path.display())));
        }
        // Windows can't hold every name git can (a trailing space, `aux.c`): said plainly, not
        // as the file system's "not found" halfway through the lab.
        if cfg!(windows)
            && let Some(why) = relative.components().find_map(|c| windows_name_problem(&c.as_os_str().to_string_lossy()))
        {
            return Err(Error::Invalid(format!("this lab can't be installed on Windows: its archive holds {}, {why}", path.display())));
        }
        let target = dest.join(&relative);
        if is_symlink {
            // A lab may rely on an internal symlink (e.g. client/src/contracts -> ../../contracts):
            // recreated when it stays inside the tree, so a link can never point outside the
            // extracted lab.
            #[cfg(unix)]
            crate::platform::tree_link::recreate_within_tree(&relative, &entry.link_name()?.unwrap_or_default(), &target)?;
        } else if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&target)?;
        }
    }
    Ok(())
}

/// Why Windows can't create a file or folder named `name` (None: it can): a name ending in a
/// space or a dot, a character Windows reserves, or a device name (`CON`, `aux.c`, `LPT1.txt`).
fn windows_name_problem(name: &str) -> Option<&'static str> {
    const DEVICES: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    if name.ends_with([' ', '.']) {
        return Some("a name ending in a space or a dot");
    }
    if name.chars().any(|c| c < ' ' || "<>:\"|?*\\".contains(c)) {
        return Some("a name with a character Windows reserves");
    }
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0");
    if DEVICES.contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
        return Some("a device name Windows reserves");
    }
    None
}

#[cfg(test)]
mod proptests;
#[cfg(test)]
pub(super) mod tests;
