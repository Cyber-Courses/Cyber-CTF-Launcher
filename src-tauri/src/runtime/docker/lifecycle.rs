//! Bringing a lab up and tearing it down.

use crate::error::Error;

/// Set (to "default") in a launch's env when the player chose the lab's default ports: each
/// service is published on its own container port (`CTFd` on 8000) instead of a random free one.
pub const PORTS_ENV: &str = "CYBERCTF_PORTS";

/// Docker's "Pool overlaps with other one on this address space", said as what to do: another
/// network (a lab, or one left behind) holds an address block this lab needs.
pub(crate) fn explain_pool_overlap(e: Error) -> Error {
    if !e.to_string().contains("Pool overlaps") {
        return e;
    }
    Error::Invalid(format!(
        "Another Docker network already uses the addresses this lab needs. Stop the other lab, or remove the network left behind (`docker network prune` removes the unused ones), then start this lab again. ({e})"
    ))
}

#[cfg(test)]
mod tests {
    use super::explain_pool_overlap;

    #[test]
    fn a_pool_overlap_says_what_to_do() {
        use crate::error::Error;
        let raw = Error::CommandFailed {
            command: "docker compose up".into(),
            stderr: "Error response from daemon: invalid pool request: Pool overlaps with other one on this address space".into(),
        };
        let said = explain_pool_overlap(raw).to_string();
        assert!(said.starts_with("Another Docker network already uses the addresses this lab needs.") && said.contains("Pool overlaps"), "{said}");
        assert_eq!(explain_pool_overlap(Error::Invalid("port taken".into())).to_string(), "port taken");
    }

    #[test]
    fn nothing_the_app_runs_listens_beyond_the_loopback() {
        // Regression (#230): the port check bound 0.0.0.0, and Windows Firewall asked the player
        // whether Cyber CTF may accept connections from public networks, mid-deploy.
        for src in crate::testing::source_scan::crate_sources() {
            for needle in ["\"0.0.0.0\"", "UNSPECIFIED", "\"::\"", "\"[::]"] {
                let binds = src.code.lines().filter(|l| l.contains("bind") && l.contains(needle)).collect::<Vec<_>>();
                assert!(binds.is_empty(), "{} binds beyond the loopback: {binds:?}", src.path.display());
            }
        }
    }
}
