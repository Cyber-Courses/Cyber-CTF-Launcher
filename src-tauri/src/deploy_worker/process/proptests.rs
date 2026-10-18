//! Property tests for the worker's pidfile and `pgrep`'s output: any text reads without a panic,
//! and what the app writes reads back the same.

use std::fmt::Write as _;

use proptest::prelude::*;

use super::*;
use crate::testing::proptest_support::config;

fn worker() -> impl Strategy<Value = Worker> {
    (any::<u32>(), proptest::option::of(any::<u64>())).prop_map(|(pid, started)| Worker { pid, started })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_pidfile_line_reads_back(w in worker(), ws in "[ \t]{0,2}", end in prop_oneof![Just(""), Just("\n"), Just("\r\n")]) {
        prop_assert_eq!(Worker::parse(&format!("{ws}{}{end}", w.line())), Some(w));
    }

    #[test]
    fn any_pidfile_text_reads_without_a_panic(text in "(?s).{0,60}") {
        if let Some(w) = Worker::parse(&text) {
            // The id is the file's first word, and a start time only ever its second.
            let mut words = text.split_whitespace();
            prop_assert_eq!(words.next().and_then(|p| p.parse::<u32>().ok()), Some(w.pid));
            prop_assert_eq!(w.started, words.next().and_then(|s| s.parse().ok()));
        } else {
            prop_assert!(text.split_whitespace().next().is_none_or(|p| p.parse::<u32>().is_err()));
        }
    }

    #[test]
    fn pgrep_ids_read_back(ids in proptest::collection::vec(any::<u32>(), 0..8), noise in proptest::collection::vec("[a-z -]{1,8}", 0..3)) {
        let mut out = String::new();
        for p in &ids {
            let _ = writeln!(out, "{p}");
        }
        for n in &noise {
            let _ = writeln!(out, "{n}");
        }
        prop_assert_eq!(pids(&out), ids);
    }
}

/// `alive` takes any pid a pidfile names; signalling it is guarded against the ids that mean
/// "a group" or "everyone" (0, 1 and anything negative as a `pid_t`).
#[cfg(unix)]
#[test]
fn pidfile_ids_that_mean_more_than_one_process_are_never_signalled() {
    for pid in [0, 1, u32::MAX, i32::MAX as u32 + 1] {
        // Would SIGTERM this test's own group, or every process of the user, if unguarded.
        signal_group(pid, 0);
    }
}
