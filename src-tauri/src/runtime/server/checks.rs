//! The outcome of "Test connection": one summary for the host row, plus a checklist for cloud
//! accounts so a failure points at the exact thing to fix.

use std::time::Instant;

use serde::Serialize;
use ts_rs::TS;

/// The outcome of a server's or cloud account's "Test connection": each check, and whether the
/// host looks usable.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerTest")]
pub struct TestResult {
    /// Everything checked passed: the host looks usable for labs.
    pub(crate) ok: bool,
    /// Host:port accepted a TCP connection.
    pub(crate) reachable: bool,
    /// Credentials verified (Proxmox API login). None when not checked (ESXi: SSH auth
    /// is only exercised by Vagrant itself).
    pub(super) authenticated: Option<bool>,
    pub(super) latency_ms: Option<u64>,
    pub(super) message: String,
    /// The individual pre-flight checks (cloud accounts only; empty for servers). The UI shows
    /// these as a checklist so a failure points at the exact thing to fix.
    #[serde(default)]
    pub(crate) checks: Vec<Check>,
}

/// One named pre-flight check in a cloud account test (e.g. "Credentials", "Terraform").
#[derive(Debug, Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerCheck")]
pub struct Check {
    /// What was checked.
    pub name: String,
    /// How it went.
    pub state: CheckState,
    /// What was found, or what to do about it.
    pub detail: String,
}

/// How a check went.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "ServerCheckState")]
pub enum CheckState {
    /// Passed.
    Ok,
    /// Not a blocker, but worth knowing (e.g. couldn't verify, or a soft limit).
    Warn,
    /// A blocker: labs won't run there until it is fixed.
    Fail,
}

impl Check {
    fn new(name: impl Into<String>, state: CheckState, detail: impl Into<String>) -> Self {
        Check { name: name.into(), state, detail: detail.into() }
    }
    /// A check `name` that passed, with `detail`.
    pub fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Check::new(name, CheckState::Ok, detail)
    }
    /// A check `name` worth knowing about, not a blocker, with `detail`.
    pub fn warn(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Check::new(name, CheckState::Warn, detail)
    }
    /// A check `name` that failed, with `detail`.
    pub fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Check::new(name, CheckState::Fail, detail)
    }
}

/// Milliseconds since `started`.
pub(super) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl TestResult {
    /// Assemble a cloud account result from its checks. `ok` unless something failed; the summary
    /// message is the first failure's detail, else the first passing check's detail.
    pub(super) fn from_checks(checks: Vec<Check>, latency_ms: Option<u64>) -> Self {
        let failed = checks.iter().find(|c| c.state == CheckState::Fail);
        let ok = failed.is_none();
        let message =
            failed.or_else(|| checks.iter().find(|c| c.state == CheckState::Ok)).or_else(|| checks.first()).map(|c| c.detail.clone()).unwrap_or_default();
        TestResult { ok, reachable: true, authenticated: Some(ok), latency_ms, message, checks }
    }

    /// A cloud account's checks, timed from `started`.
    pub(super) fn timed(checks: Vec<Check>, started: Instant) -> Self {
        TestResult::from_checks(checks, Some(elapsed_ms(started)))
    }

    /// This result with one more check: whether it passed and what it says are read again from
    /// every check, by `from_checks`' rule, so a failing check added after the others can't leave
    /// a failed result saying what passed.
    pub(super) fn with_check(mut self, check: Check) -> Self {
        self.checks.push(check);
        let (reachable, latency_ms) = (self.reachable, self.latency_ms);
        TestResult { reachable, ..TestResult::from_checks(self.checks, latency_ms) }
    }

    /// A cloud account whose service (or CLI) couldn't be reached at all.
    pub(super) fn unreachable_checks(checks: Vec<Check>) -> Self {
        // Unreachable is never ok, and nothing was authenticated, whatever the checks say.
        TestResult { ok: false, reachable: false, authenticated: None, ..TestResult::from_checks(checks, None) }
    }

    /// A server that didn't accept a connection.
    pub(super) fn unreachable(message: String) -> Self {
        TestResult { ok: false, reachable: false, authenticated: None, latency_ms: None, message, checks: Vec::new() }
    }

    /// A server set up with an SSH key: the setup's own checks (`before`), then this result as
    /// the API's check. Ok only when every one is; the message is the first failure's.
    pub(crate) fn with_sign_in_checks(mut self, before: Vec<Check>) -> Self {
        let api = if self.ok { Check::ok("Proxmox API", self.message.clone()) } else { Check::fail("Proxmox API", self.message.clone()) };
        let mut checks = before;
        checks.push(api);
        if let Some(failed) = checks.iter().find(|c| c.state == CheckState::Fail) {
            self.ok = false;
            self.message.clone_from(&failed.detail);
        }
        self.checks = checks;
        self
    }

    /// A server that answered (checks are cloud-only, and an SSH-key Proxmox setup's).
    pub(super) fn reached(ok: bool, authenticated: Option<bool>, latency_ms: Option<u64>, message: impl Into<String>) -> Self {
        TestResult { ok, reachable: true, authenticated, latency_ms, message: message.into(), checks: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_added_last_counts_as_the_others_do() {
        // Regression: a GCP account whose labs project couldn't be made read as failed (a red
        // cross) while it said "Signed in; billing account …", the first check's pass.
        let signed_in = || TestResult::from_checks(vec![Check::ok("Sign-in", "Signed in; billing account \"Main\"."), Check::ok("Terraform", "1.9")], Some(5));
        let r = signed_in().with_check(Check::fail("Labs project", "no free project slot on the billing account"));
        assert!(!r.ok);
        assert_eq!(
            (r.message.as_str(), r.authenticated, r.latency_ms, r.checks.len()),
            ("no free project slot on the billing account", Some(false), Some(5), 3)
        );
        let r = signed_in().with_check(Check::ok("Labs project", "Labs run in the project p."));
        assert!(r.ok);
        assert_eq!(r.message, "Signed in; billing account \"Main\".");
    }

    #[test]
    fn test_result_from_checks_summarises_first_failure() {
        let fail = vec![Check::ok("A", "a ok"), Check::fail("B", "b broke"), Check::warn("C", "c warn")];
        let r = TestResult::from_checks(fail, Some(5));
        assert!(!r.ok);
        assert_eq!(r.authenticated, Some(false));
        assert_eq!(r.message, "b broke");
        // All-pass: ok, message is the first passing detail.
        let pass = vec![Check::ok("A", "a ok"), Check::warn("C", "c warn")];
        let r = TestResult::from_checks(pass, None);
        assert!(r.ok && r.authenticated == Some(true));
        assert_eq!(r.message, "a ok");
        // Only warnings: still ok, summarised by the first one.
        let r = TestResult::from_checks(vec![Check::warn("W", "w1"), Check::warn("W", "w2")], None);
        assert!(r.ok);
        assert_eq!(r.message, "w1");
    }

    #[test]
    fn an_unreachable_account_with_only_warnings_is_not_ok() {
        // The callers today always push a failing check first; `unreachable_checks` itself
        // should still never say ok about a cloud it couldn't reach.
        let r = TestResult::unreachable_checks(vec![Check::warn("CLI", "couldn't tell")]);
        assert!(!r.reachable);
        assert!(!r.ok, "unreachable is never ok");
        assert_ne!(r.authenticated, Some(true));
        let r = TestResult::unreachable_checks(Vec::new());
        assert!(!r.ok && r.message.is_empty());
    }

    #[test]
    fn an_ssh_key_setup_lists_each_part_before_the_api() {
        let api = TestResult::reached(true, Some(true), Some(4), "Connected to pve");
        let r = api.with_sign_in_checks(vec![Check::ok("SSH with your key", "signed in"), Check::ok("API token", "made")]);
        assert!(r.ok && r.message == "Connected to pve");
        assert_eq!(r.checks.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["SSH with your key", "API token", "Proxmox API"]);
        // A failed part fails the test and says why, even when the API answers.
        let r = TestResult::reached(true, Some(true), None, "ok").with_sign_in_checks(vec![Check::fail("SSH with your key", "denied")]);
        assert!(!r.ok && r.message == "denied");
        let r = TestResult::reached(false, Some(false), None, "rejected").with_sign_in_checks(vec![Check::ok("SSH with your key", "fine")]);
        assert!(!r.ok && r.message == "rejected" && r.checks[1].state == CheckState::Fail);
    }

    #[test]
    fn unreachable_results_are_not_ok() {
        let r = TestResult::unreachable_checks(vec![Check::fail("API token", "can't reach")]);
        assert!(!r.ok && !r.reachable && r.latency_ms.is_none());
        assert_eq!(r.message, "can't reach");
        let r = TestResult::unreachable("Timed out".into());
        assert!(!r.ok && !r.reachable && r.authenticated.is_none() && r.checks.is_empty());
    }
}
