//! The wall clock as Unix time, read one way everywhere: a clock set before 1970 reads as the epoch.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long after the Unix epoch it is now; zero on a clock set before it.
pub fn since_epoch() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

/// Whole seconds since the Unix epoch, now.
pub fn unix_secs() -> u64 {
    since_epoch().as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_reads_as_seconds_since_1970() {
        // 2024-01-01T00:00:00Z: any clock this runs on is past it.
        assert!(unix_secs() > 1_704_067_200);
        assert_eq!(since_epoch().as_secs() / 60, unix_secs() / 60);
    }
}
