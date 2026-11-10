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
            // recreate one that stays inside the tree, leave out one that is absolute or climbs out
            // of it, so a link can never point outside the extracted lab.
            let link = entry.link_name()?.map(std::borrow::Cow::into_owned).unwrap_or_default();
            if symlink_target_within_tree(&relative, &link) {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                recreate_symlink(&link, &target)?;
            }
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

/// Lexically, does a symlink at `link_rel` (its path relative to the tree root) that points at
/// `target` resolve to a location still inside the tree? An absolute target, or one that climbs
/// above the root with `..`, is rejected, so an internal relative link is kept while one that would
/// escape the extracted lab is left out. Purely lexical: it never touches the file system.
pub(crate) fn symlink_target_within_tree(link_rel: &Path, target: &Path) -> bool {
    use std::path::Component;
    if target.is_absolute() {
        return false;
    }
    // Depth of the link's own folder below the root; each `..` in the target climbs one level, and
    // climbing above the root (depth already 0) means the target escapes the tree.
    let mut depth: usize = link_rel.parent().map_or(0, |p| p.components().filter(|c| matches!(c, Component::Normal(_))).count());
    for c in target.components() {
        match c {
            Component::ParentDir => match depth.checked_sub(1) {
                Some(d) => depth = d,
                None => return false,
            },
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            // A root or prefix component means an absolute/Windows-rooted target: rejected.
            _ => return false,
        }
    }
    true
}

/// Recreates a symlink to `target` at `at`. Off Unix the link is left out (a published lab builds
/// its image on Linux, where the symlink is what matters), the same as before this was added.
#[cfg(unix)]
pub(crate) fn recreate_symlink(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}
#[cfg(not(unix))]
#[expect(clippy::unnecessary_wraps, reason = "the Unix version's signature: one call site for both")]
pub(crate) fn recreate_symlink(_target: &Path, _at: &Path) -> std::io::Result<()> {
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
