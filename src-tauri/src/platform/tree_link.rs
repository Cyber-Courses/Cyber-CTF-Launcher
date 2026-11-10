//! A symlink found in a tree being copied (a lab's archive, a lab folder imported from disk): kept
//! only when it stays inside the tree, so a copy never points outside itself. Unix only: off Unix
//! a lab's links are left out (a published lab builds its image on Linux, where they matter).

use std::path::Path;

/// Lexically, does a symlink at `link_rel` (its path relative to the tree root) that points at
/// `target` resolve to a location still inside the tree? An absolute target, or one that climbs
/// above the root with `..`, is rejected, so an internal relative link is kept while one that would
/// escape the tree is left out. Purely lexical: it never touches the file system.
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

/// Recreates at `at` the symlink found at `link_rel` (its path relative to the tree's root) to
/// `target`, when it stays inside the tree ([`symlink_target_within_tree`]); one that would
/// escape it is left out. The link is recreated, not followed, so a dangling one is harmless.
#[cfg(unix)]
pub(crate) fn recreate_within_tree(link_rel: &Path, target: &Path, at: &Path) -> std::io::Result<()> {
    if !symlink_target_within_tree(link_rel, target) {
        return Ok(());
    }
    if let Some(parent) = at.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(target, at)
}
