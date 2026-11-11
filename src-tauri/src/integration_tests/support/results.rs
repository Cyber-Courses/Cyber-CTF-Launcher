//! The results an entry point the test rig calls writes for it: one JSON record per check,
//! appended as JSON Lines to the file `CYBERCTF_TEST_RESULTS` names, in the format every test
//! runner writes (tests/README.md, "Results"):
//!
//! ```text
//! {"suite":"setup","case":"docker","provider":"libvirt","form":"","result":"pass","reason":"installed: Docker installed","seconds":42,"log":"…"}
//! ```

use std::io::Write as _;
use std::path::Path;

/// The knob naming the file the records are appended to.
pub(crate) const RESULTS: &str = "CYBERCTF_TEST_RESULTS";

/// A check's verdict.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Verdict {
    Pass,
    Fail,
    Skip,
}

/// One check's record, its fields in the contract's order.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize)]
pub(crate) struct Record {
    /// The entry point: `setup`.
    pub(crate) suite: String,
    /// The check: a scenario, a spec, an item.
    pub(crate) case: String,
    /// The hypervisor, cloud or server it ran on; empty where none applies.
    pub(crate) provider: String,
    /// The form the lab ran in; empty where none applies.
    pub(crate) form: String,
    pub(crate) result: Verdict,
    /// Why it failed or was skipped, or what it did.
    pub(crate) reason: String,
    pub(crate) seconds: u64,
    /// Where its log is; empty when it has none of its own.
    pub(crate) log: String,
}

impl Record {
    /// The record as its line: one line of JSON, whatever its text holds.
    pub(crate) fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| panic!("a record serializes: {e}"))
    }
}

/// Appends `record` to `file` at once, so a crash later in the run still leaves it.
pub(crate) fn append(file: &Path, record: &Record) {
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("creating {}: {e}", dir.display()));
    }
    let appended = std::fs::OpenOptions::new().create(true).append(true).open(file).and_then(|mut f| writeln!(f, "{}", record.line()));
    if let Err(e) = appended {
        panic!("appending to {}: {e}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(reason: &str) -> Record {
        Record {
            suite: "setup".into(),
            case: "docker".into(),
            provider: "libvirt".into(),
            form: String::new(),
            result: Verdict::Pass,
            reason: reason.into(),
            seconds: 42,
            log: "setup-logs".into(),
        }
    }

    #[test]
    fn a_record_is_one_line_of_the_contracts_fields_in_order() {
        assert_eq!(
            record("installed: Docker installed").line(),
            r#"{"suite":"setup","case":"docker","provider":"libvirt","form":"","result":"pass","reason":"installed: Docker installed","seconds":42,"log":"setup-logs"}"#
        );
        // A note on several lines, with quotes, stays one line of valid JSON.
        let line = record("exited with 1:\n\t\"E: no\"").line();
        assert!(!line.contains('\n'));
        let back: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(back["reason"], "exited with 1:\n\t\"E: no\"");
        assert_eq!(serde_json::to_value(Verdict::Skip).unwrap(), "skip");
    }

    #[test]
    fn records_are_appended_a_line_each() {
        let t = crate::testing::test_app::TestApp::new();
        let file = t.data().join("out").join("results.jsonl");
        append(&file, &record("one"));
        append(&file, &Record { result: Verdict::Fail, ..record("two") });
        let text = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0]["reason"].as_str(), lines[1]["result"].as_str()), (Some("one"), Some("fail")));
    }
}
