//! The runner's own words the launcher still reads, in one place: Isothrum 2.x says some failures
//! only in its English text, with no code or field of their own. Each reading here is checked
//! against the runner's exact wording (its fixture), so a runner release that rewords one fails
//! a test here rather than a player's start.
//!
//! Isothrum 3.0 gives each of them a code (`tool_unanswered` for a tools-phase probe that failed
//! or answered nothing, with the tool's name in its own field); once the launcher pins it, each
//! function here becomes a match on that code and this module goes.

/// The step the runner probes the run's tools in.
const TOOLS_STEP: &str = "tools";

/// How the runner begins the reason of a tools-phase Docker probe that failed or printed nothing.
const DOCKER_UNANSWERED: &str = "`docker` doesn't answer";

/// Whether step `step`'s failure `reason` is the runner's tools probe of Docker that got no
/// answer (the engine stopped, or this account refused it: Docker's own words follow).
pub(super) fn docker_unanswered(step: &str, reason: &str) -> bool {
    step == TOOLS_STEP && reason.starts_with(DOCKER_UNANSWERED)
}

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

    /// The runner's texts, as Isothrum 2.1.7's `tools.rs` and `executor` write them.
    #[test]
    fn the_runners_words_are_read_as_it_writes_them() {
        assert!(docker_unanswered(
            "tools",
            "`docker` doesn't answer: Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running? (exited with code 1)"
        ));
        assert!(docker_unanswered("tools", "`docker` doesn't answer: its probe printed nothing (its daemon isn't running)"));
        assert!(!docker_unanswered("up", "`docker` doesn't answer: x"), "only the tools probe");
        assert!(!docker_unanswered("tools", "`vagrant` doesn't answer: x"));
        assert!(is_box_download("boxes-up") && !is_box_download("up"));
    }
}
