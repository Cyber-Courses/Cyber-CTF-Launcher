//! Property tests for the lab host's bootstrap report: any first line is a step, ready or failed.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

proptest! {
    #![proptest_config(config())]

    #[test]
    fn any_report_line_reads_as_a_state(line in "(?s).{0,80}") {
        let want = match line.trim() {
            "ready" => Progress::Ready,
            l => match l.strip_prefix("failed:") {
                Some(step) => Progress::Failed(step.trim().to_string()),
                None => Progress::Running(l.strip_prefix("running:").unwrap_or("booting").trim().to_string()),
            },
        };
        prop_assert_eq!(parse_progress(&line), want);
    }

    #[test]
    fn steps_round_trip(step in "[^\\s](\\PC{0,40}[^\\s])?", ws in "[ \t\r\n]{0,2}") {
        prop_assert_eq!(parse_progress(&format!("{ws}running: {step}{ws}")), Progress::Running(step.clone()));
        prop_assert_eq!(parse_progress(&format!("failed:{step}{ws}")), Progress::Failed(step));
    }
}
