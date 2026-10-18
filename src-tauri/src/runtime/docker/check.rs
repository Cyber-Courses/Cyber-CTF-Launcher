//! The lab's self-check: its Isoloom `checks:` (the lab's own scripts and declared probes, and
//! the ones Isoloom derives from `services` and `reach`), run by the Compose file's check
//! runners (profile `check`), one per position, read into a per-check table.

/// One check's outcome.
#[derive(serde::Serialize, Clone, Debug, PartialEq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabCheckResult")]
pub struct CheckResult {
    pub name: String,
    /// Where it ran: "from web", or "from the environment's networks".
    pub from: String,
    pub ok: bool,
    /// Why it failed (empty when it passed).
    pub reason: String,
}

/// Result of a lab's self-verification.
#[derive(serde::Serialize, Debug, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabCheck")]
pub struct Check {
    /// The lab has checks (its own, or derived from its spec), so verification is possible.
    pub available: bool,
    /// Every check passed: the lab behaves, and the challenge is still in a solvable state.
    pub ok: bool,
    /// The runners' combined output.
    pub output: String,
    /// Every check, in the order it ran.
    pub results: Vec<CheckResult>,
}
