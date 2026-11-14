//! What `ping` prints, read: whether the address answered. Its fixtures are in `fixtures/ping/`.
//!
//! Windows' `ping.exe` exits 0 when a router answers `Destination host unreachable` for the
//! address, so its exit code says nothing: a reply is a line with the reply's `TTL=` (`ttl=` on
//! Linux and macOS), which every language's `ping` prints and an unreachable answer never does.

/// What one `ping` said of its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ping {
    /// The address answered.
    Reply,
    /// Nothing answered for it: no reply in time, or a router saying it is unreachable.
    NoReply,
}

impl Ping {
    /// What a `ping` that printed `out` (its standard output, when it exited 0) said.
    #[must_use]
    pub fn of(out: &str) -> Ping {
        if out.lines().any(|l| l.to_ascii_lowercase().contains("ttl=")) { Ping::Reply } else { Ping::NoReply }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_read_as_each_ping_prints_them() {
        for (fixture, said) in [
            (include_str!("fixtures/ping/iputils-20240117-reply.txt"), Ping::Reply),
            (include_str!("fixtures/ping/macos-15-reply.txt"), Ping::Reply),
            (include_str!("fixtures/ping/windows-11-reply.txt"), Ping::Reply),
            (include_str!("fixtures/ping/windows-11-fr-reply.txt"), Ping::Reply),
            // Regression (#370): exit 0, and no answer from the address.
            (include_str!("fixtures/ping/windows-11-unreachable.txt"), Ping::NoReply),
            (include_str!("fixtures/ping/windows-11-fr-unreachable.txt"), Ping::NoReply),
            (include_str!("fixtures/ping/windows-11-timeout.txt"), Ping::NoReply),
            ("", Ping::NoReply),
        ] {
            assert_eq!(Ping::of(fixture), said, "{fixture}");
        }
    }
}
