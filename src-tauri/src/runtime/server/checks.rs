//! The outcome of "Test connection": one summary for the host row, plus a checklist for cloud
//! accounts so a failure points at the exact thing to fix.

use std::time::Instant;

use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerTest")]
pub struct TestResult {
    /// Everything checked passed: the host looks usable for labs.
    pub(super) ok: bool,
    /// Host:port accepted a TCP connection.
    pub(super) reachable: bool,
    /// Credentials verified (Proxmox API login). None when not checked (ESXi: SSH auth
    /// is only exercised by Vagrant itself).
    pub(super) authenticated: Option<bool>,
    pub(super) latency_ms: Option<u64>,
    pub(super) message: String,
    /// The individual pre-flight checks (cloud accounts only; empty for servers). The UI shows
    /// these as a checklist so a failure points at the exact thing to fix.
    #[serde(default)]
    pub(super) checks: Vec<Check>,
}

/// One named pre-flight check in a cloud account test (e.g. "Credentials", "Terraform").
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerCheck")]
pub struct Check {
    pub name: String,
    pub state: CheckState,
    pub detail: String,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "ServerCheckState")]
pub enum CheckState {
    Ok,
    /// Not a blocker, but worth knowing (e.g. couldn't verify, or a soft limit).
    Warn,
    Fail,
}

impl Check {
    fn new(name: impl Into<String>, state: CheckState, detail: impl Into<String>) -> Self {
        Check { name: name.into(), state, detail: detail.into() }
    }
    pub fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Check::new(name, CheckState::Ok, detail)
    }
    pub fn warn(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Check::new(name, CheckState::Warn, detail)
    }
    pub fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Check::new(name, CheckState::Fail, detail)
    }
}

/// Milliseconds since `started`.
pub(super) fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
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

    /// A cloud account whose service (or CLI) couldn't be reached at all.
    pub(super) fn unreachable_checks(checks: Vec<Check>) -> Self {
        // Unreachable is never ok, and nothing was authenticated, whatever the checks say.
        TestResult { ok: false, reachable: false, authenticated: None, ..TestResult::from_checks(checks, None) }
    }

    /// A server that didn't accept a connection.
    pub(super) fn unreachable(message: String) -> Self {
        TestResult { ok: false, reachable: false, authenticated: None, latency_ms: None, message, checks: Vec::new() }
    }

    /// A server that answered (checks are cloud-only).
    pub(super) fn reached(ok: bool, authenticated: Option<bool>, latency_ms: Option<u64>, message: impl Into<String>) -> Self {
        TestResult { ok, reachable: true, authenticated, latency_ms, message: message.into(), checks: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn unreachable_results_are_not_ok() {
        let r = TestResult::unreachable_checks(vec![Check::fail("API token", "can't reach")]);
        assert!(!r.ok && !r.reachable && r.latency_ms.is_none());
        assert_eq!(r.message, "can't reach");
        let r = TestResult::unreachable("Timed out".into());
        assert!(!r.ok && !r.reachable && r.authenticated.is_none() && r.checks.is_empty());
    }
}
