//! What Homebrew prints when it, not the formula, is what is broken, read from one line of its
//! error output: the player's environment to repair, which the installer can't mend itself.

/// A broken Homebrew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// A lock it can't take: a root-owned update lock (after a `sudo brew`), or a stale one it
    /// thinks another update holds.
    Locked,
    /// Too old to know this macOS (`unknown or unsupported macOS version: :dunno`).
    Outdated,
    /// A source build refused because the installed Xcode is older than it needs.
    XcodeTooOld,
}

impl Trouble {
    /// The trouble `line` says, if it says one.
    #[must_use]
    pub fn of_line(line: &str) -> Option<Trouble> {
        if (line.contains("homebrew/locks") && line.contains("Permission denied")) || (line.contains("brew update") && line.contains("already running")) {
            Some(Trouble::Locked)
        } else if line.contains("unknown or unsupported macOS version") {
            Some(Trouble::Outdated)
        } else if line.contains("is too outdated") || line.contains("Please update to Xcode") {
            Some(Trouble::XcodeTooOld)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homebrews_lines_are_read_as_it_prints_them() {
        assert_eq!(Trouble::of_line("Error: Another `brew update` process is already running."), Some(Trouble::Locked));
        assert_eq!(Trouble::of_line("Error: unknown or unsupported macOS version: :dunno"), Some(Trouble::Outdated));
        assert_eq!(Trouble::of_line("Error: Your Xcode (26.6) at /Applications/Xcode.app is too outdated."), Some(Trouble::XcodeTooOld));
        assert_eq!(Trouble::of_line("==> Fetching qemu"), None);
    }
}
