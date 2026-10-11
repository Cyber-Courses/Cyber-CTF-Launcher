//! Property tests for following a deploy worker's log: any log yields a short step, and the
//! lines come out the same however the reads cut the file.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

/// Log text as the worker writes it: Vagrant action lines, progress chatter, the launcher's own
/// lines and marks, non-ASCII.
fn log_text() -> impl Strategy<Value = String> {
    let line = prop_oneof![
        "==> [a-z0-9]{1,6}: \\PC{0,100}",
        "==> [a-z0-9]{0,6}:? ?",
        "    [a-z0-9]{1,6}: [0-9]{1,3}%",
        "(✓|✗) \\PC{0,20}",
        "[ \t]{0,3}",
        "\\PC{0,120}",
    ];
    proptest::collection::vec(line, 0..12).prop_map(|ls| ls.join("\n"))
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn the_step_is_short_and_never_a_mark_or_blank(log in log_text()) {
        if let Some(step) = last_step(&log) {
            prop_assert!(step.chars().count() <= STEP_MAX, "{} chars", step.chars().count());
            prop_assert!(!step.trim().is_empty());
            prop_assert!(!step.starts_with('✓') && !step.starts_with('✗'));
        }
    }

    #[test]
    fn last_step_never_panics(log in "(?s).{0,400}") {
        let _ = last_step(&log);
    }

    #[test]
    fn only_ok_is_a_success(status in "(?s).{0,40}") {
        prop_assert_eq!(verdict(&status).is_ok(), status.trim() == "ok");
    }

    /// Reads cut the log anywhere, even inside a multi-byte character: the lines that come out
    /// are the file's lines, whole and undamaged.
    #[test]
    fn lines_come_out_whole_however_the_reads_cut(log in log_text(), cuts in proptest::collection::vec(any::<prop::sample::Index>(), 0..8)) {
        let bytes = log.as_bytes();
        let mut at: Vec<usize> = cuts.iter().map(|c| c.index(bytes.len() + 1)).collect();
        at.push(0);
        at.push(bytes.len());
        at.sort_unstable();
        at.dedup();
        let mut buf = LineBuffer::default();
        let mut out = Vec::new();
        for w in at.windows(2) {
            buf.push(&bytes[w[0]..w[1]], |l| out.push(l));
        }
        out.extend(buf.rest());
        let want: Vec<String> = log.split('\n').map(str::to_string).filter(|_| !log.is_empty()).collect();
        // A trailing newline ends the last line rather than starting an empty one.
        let want: Vec<String> = if log.ends_with('\n') { want[..want.len() - 1].to_vec() } else { want };
        prop_assert_eq!(out, want);
    }
}
