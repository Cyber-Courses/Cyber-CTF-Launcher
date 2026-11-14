//! The runner's own words the launcher still reads, in one place: the runner names a Vagrant
//! start's box download only by its step's name, with no code or field of its own. Each reading
//! here is checked against the runner's exact wording, so a runner release that renames it fails
//! a test here rather than a player's start.

/// The prefix the runner names a Vagrant start's box download step with (`boxes-up`, after the
/// step it downloads for).
const BOX_DOWNLOAD: &str = "boxes-";

/// Whether a download step is the one that fetches the boxes a Vagrant start boots.
pub(super) fn is_box_download(step: &str) -> bool {
    step.starts_with(BOX_DOWNLOAD)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The step names as the runner's downloads write them.
    #[test]
    fn the_runners_words_are_read_as_it_writes_them() {
        assert!(is_box_download("boxes-up") && !is_box_download("up") && !is_box_download("pull-up"));
    }
}
