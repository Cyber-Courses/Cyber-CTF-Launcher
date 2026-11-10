//! A symlink found in a tree being copied (a lab's archive, a lab folder imported from disk): kept
//! only when it stays inside the tree, so a copy never points outside itself. Judged twice: by
//! its path ([`symlink_target_within_tree`]), and, once the rest of the tree is in place, on disk
//! ([`TreeLinks::recreate`]), where a chain of links shows what a path alone can't (`sub/a -> ..`
//! then `b -> sub/a/../secret` reads as inside, and resolves above the tree). Unix only: off Unix
//! a lab's links are left out (a published lab builds its image on Linux, where they matter).

use std::path::{Component, Path, PathBuf};

/// Lexically, does a symlink at `link_rel` (its path relative to the tree root) that points at
/// `target` resolve to a location still inside the tree? An absolute target, or one that climbs
/// above the root with `..`, is rejected, so an internal relative link is kept while one that would
/// escape the tree is left out. Purely lexical: it never touches the file system.
pub(crate) fn symlink_target_within_tree(link_rel: &Path, target: &Path) -> bool {
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

/// The symlinks of a tree being copied, held back until the rest of the tree is in place: each is
/// then recreated only when what it points at, resolved on disk through any link made before it,
/// lies inside the tree.
#[derive(Debug, Default)]
pub(crate) struct TreeLinks {
    /// Each link's path relative to the tree's root, and its target as written.
    links: Vec<(PathBuf, PathBuf)>,
}

impl TreeLinks {
    /// Notes the symlink at `link_rel` (relative to the tree's root) to `target`, for
    /// [`TreeLinks::recreate`].
    pub(crate) fn push(&mut self, link_rel: &Path, target: &Path) {
        self.links.push((link_rel.to_path_buf(), target.to_path_buf()));
    }

    /// Recreates under `root` every link that stays inside the tree; the others are left out, and
    /// said so in the log. A link is made once what it points at resolves (`canonicalize`) inside
    /// `root`, so a link through another link gets its turn after that one; what still can't be
    /// resolved inside the tree at the end (dangling into a folder that isn't there, or escaping
    /// it) is left out. Off Unix every link is left out.
    pub(crate) fn recreate(self, root: &Path) -> std::io::Result<()> {
        if self.links.is_empty() {
            return Ok(());
        }
        let (waiting, escaping): (Vec<_>, Vec<_>) = self.links.into_iter().partition(|(rel, target)| symlink_target_within_tree(rel, target));
        for (link_rel, _) in escaping {
            log::warn!("{}: a symlink pointing outside the lab's folder, left out", link_rel.display());
        }
        make(waiting, root)
    }
}

/// Makes under `root` the links of `waiting` (each inside the tree by its path) whose target
/// resolves on disk inside `root`. A pass makes every link whose target resolves; one through a
/// link made in that pass waits for the next. A pass that made none leaves the rest out.
#[cfg(unix)]
fn make(mut waiting: Vec<(PathBuf, PathBuf)>, root: &Path) -> std::io::Result<()> {
    let real_root = root.canonicalize()?;
    while !waiting.is_empty() {
        let mut later = Vec::new();
        let mut made = false;
        for (link_rel, target) in waiting {
            let at = root.join(&link_rel);
            match resolved_inside(&at, &target, &real_root) {
                Some(true) => {
                    if let Some(parent) = at.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::os::unix::fs::symlink(&target, &at)?;
                    made = true;
                }
                Some(false) => log::warn!("{}: a symlink pointing outside the lab's folder, left out", link_rel.display()),
                None => later.push((link_rel, target)),
            }
        }
        if !made {
            for (link_rel, _) in later {
                log::warn!("{}: a symlink to nothing in the lab's folder, left out", link_rel.display());
            }
            break;
        }
        waiting = later;
    }
    Ok(())
}

/// Off Unix a lab's links are left out: `root` must still be there, as on Unix.
#[cfg(not(unix))]
fn make(waiting: Vec<(PathBuf, PathBuf)>, root: &Path) -> std::io::Result<()> {
    std::fs::metadata(root)?;
    for (link_rel, _) in waiting {
        log::debug!("{}: a symlink, left out off Unix", link_rel.display());
    }
    Ok(())
}

/// Whether the link at `at` to `target` resolves inside `real_root` (canonical): `None` when what
/// it points at isn't there yet (a folder another link makes, or nothing at all).
#[cfg(unix)]
fn resolved_inside(at: &Path, target: &Path, real_root: &Path) -> Option<bool> {
    let full = at.parent()?.join(target);
    if let Ok(real) = full.canonicalize() {
        return Some(real.starts_with(real_root));
    }
    // Dangling: its folder must resolve inside, and its last step be a plain name there (`a/..`
    // past a link `a` would be judged by the folder alone otherwise).
    let Component::Normal(last) = target.components().next_back()? else {
        return None;
    };
    let folder = full.parent()?.canonicalize().ok()?;
    Some(folder.starts_with(real_root) && folder.join(last).starts_with(real_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_stays_inside_the_tree_by_its_path_or_is_left_out() {
        let within = |link: &str, target: &str| symlink_target_within_tree(Path::new(link), Path::new(target));
        assert!(within("client/src/contracts", "../../contracts"));
        assert!(within("a/b", "c"));
        assert!(within("a/b", "./c"));
        assert!(within("top", "sub/file"));
        assert!(!within("top", ".."));
        assert!(!within("a/b", "../../.."));
        assert!(!within("a/b", "/etc/passwd"));
        assert!(!within("a/b", "../../../etc"));
    }

    /// A fresh folder with `secret` beside it (outside the tree) and the tree at `lab`.
    #[cfg(unix)]
    fn outside_and_tree() -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("cyberctf-tree-link-{}", rand::random::<u32>()));
        let lab = dir.join("lab");
        std::fs::create_dir_all(lab.join("sub")).unwrap();
        std::fs::write(dir.join("secret"), "s").unwrap();
        std::fs::write(lab.join("sub/inside.txt"), "i").unwrap();
        (dir, lab)
    }

    #[cfg(unix)]
    #[test]
    fn a_chain_of_links_that_resolves_outside_the_tree_is_left_out() {
        // Regression: judged by its path alone, `b -> sub/a/../secret` looked inside the tree
        // (`sub/a/..` reads as `sub`), while `sub/a -> ..` made it resolve beside the lab.
        let (dir, lab) = outside_and_tree();
        let mut links = TreeLinks::default();
        links.push(Path::new("sub/a"), Path::new(".."));
        links.push(Path::new("b"), Path::new("sub/a/../secret"));
        links.push(Path::new("c"), Path::new("sub/a/.."));
        links.push(Path::new("ok"), Path::new("sub/a/sub/inside.txt"));
        links.recreate(&lab).unwrap();
        assert!(lab.join("sub/a").is_symlink(), "an internal link is kept");
        assert!(!lab.join("b").exists() && !lab.join("b").is_symlink(), "the chain to the secret is left out");
        assert!(!lab.join("c").is_symlink(), "a chain to the tree's parent is left out");
        assert_eq!(std::fs::read_to_string(lab.join("ok")).unwrap(), "i", "a chain that stays inside is kept");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_through_a_link_made_later_gets_its_turn_and_a_dangling_one_stays_in_its_folder() {
        let (dir, lab) = outside_and_tree();
        let mut links = TreeLinks::default();
        links.push(Path::new("via"), Path::new("alias/inside.txt"));
        links.push(Path::new("alias"), Path::new("sub"));
        links.push(Path::new("sub/later"), Path::new("made-at-run-time"));
        links.push(Path::new("sub/nowhere"), Path::new("no-such-folder/x"));
        links.recreate(&lab).unwrap();
        assert_eq!(std::fs::read_to_string(lab.join("via")).unwrap(), "i", "resolved once `alias` was made");
        assert_eq!(std::fs::read_link(lab.join("sub/later")).unwrap(), Path::new("made-at-run-time"), "dangling inside its own folder: kept");
        assert!(!lab.join("sub/nowhere").is_symlink(), "dangling into a folder that isn't there: left out");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
