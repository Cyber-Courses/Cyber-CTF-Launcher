//! How far each launch path has been proven, from Isoloom's tier evidence (`isoloom-targets`'s
//! bundled `tiers.json`), for each of a lab's run options (`run_options`). The launcher shows
//! a badge off the evidence's runs, not the tier number: a path no host OS's latest run has passed
//! (validated only) is Beta, whatever its tier; a path real runs have passed (AWS, Docker, the
//! proven VM providers…) carries no badge even while tier 1 waits for the other host OSes. A path
//! the evidence doesn't have, or evidence that can't be read, is Beta: nothing proves it. This
//! updates itself as the evidence does — no hardcoded list.

use isoloom_targets::tiers::{self, TierEntry};

/// The badge a launch path carries: none when real runs prove it, Beta when it is only validated,
/// Experimental when the target itself is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum Badge {
    /// Real runs have passed: no badge.
    None,
    /// Generated and validated, but no real run yet.
    Beta,
    /// The target is experimental.
    Experimental,
}

/// One launch path's maturity: the Isoloom target and its provider/cloud (`docker`, `cloud-vm`
/// with `aws`…), the badge, and the evidence line behind it (the badge's tooltip).
#[derive(Debug, Clone)]
pub struct RunTier {
    /// The Isoloom target id (`docker`, `vagrant`, `cloud-vm`, `cloud-docker`, …).
    pub target: String,
    /// Its provider or cloud (`aws`, `virtualbox`, …), or `None` for a target with one form.
    pub variant: Option<String>,
    /// What the launcher shows for it.
    pub badge: Badge,
    /// The evidence line (`real runs: …`, `validated only; tier 1 pending: …`), for the tooltip.
    pub evidence: String,
}

/// The badge entry `e` earns: Experimental for an experimental target, Beta while no host OS's
/// latest run has passed (validated only, or only failed runs), else None.
fn badge_of(e: &TierEntry) -> Badge {
    if e.experimental {
        Badge::Experimental
    } else if e.proven().is_empty() {
        Badge::Beta
    } else {
        Badge::None
    }
}

/// Every launch path's maturity, from Isoloom's tier evidence. Empty, and logged, when the
/// evidence can't be read: each path then reads as unproven (`badge_or_beta`), never as proven.
pub fn run_tiers() -> Vec<RunTier> {
    match tiers::tiers() {
        Ok(entries) => entries
            .iter()
            .map(|e| RunTier { target: e.target.id().to_owned(), variant: e.variant.map(str::to_owned), badge: badge_of(e), evidence: e.evidence() })
            .collect(),
        Err(e) => {
            log::warn!("Isoloom's tier evidence can't be read, every launch path reads as Beta: {e}");
            Vec::new()
        }
    }
}

/// The badge of `tier`, the evidence's entry for a launch path: Beta when the evidence has none
/// for it (logged), as nothing proves it.
pub fn badge_or_beta(tier: Option<&RunTier>, path: &str) -> Badge {
    tier.map_or_else(
        || {
            log::warn!("no tier evidence for launch path {path}: it reads as Beta");
            Badge::Beta
        },
        |t| t.badge,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The evidence's entry for `target`/`variant`.
    fn entry(target: &str, variant: &str) -> TierEntry {
        tiers::tiers().unwrap().into_iter().find(|e| e.target.id() == target && e.variant == Some(variant)).unwrap()
    }

    #[test]
    fn the_runs_not_the_evidence_text_decide_the_badge() {
        // Regression: the badge parsed the evidence line's English ("validated only", "experimental"),
        // so a failed latest run, or a rewording, read as proven.
        let mut aws = entry("cloud-vm", "aws");
        assert_eq!(badge_of(&aws), Badge::None, "real runs passed");
        for run in &mut aws.runs {
            run.result = tiers::RunResult::Fail;
        }
        assert_eq!(badge_of(&aws), Badge::Beta, "its runs failed: {}", aws.evidence());
        aws.runs.clear();
        assert_eq!(badge_of(&aws), Badge::Beta, "validated only");
        aws.experimental = true;
        assert_eq!(badge_of(&aws), Badge::Experimental);
    }

    #[test]
    fn a_path_missing_from_the_evidence_is_beta_never_proven() {
        assert_eq!(badge_or_beta(None, "docker-vm/virtualbox"), Badge::Beta);
        let proven = RunTier { target: "docker".into(), variant: None, badge: Badge::None, evidence: String::new() };
        assert_eq!(badge_or_beta(Some(&proven), "docker"), Badge::None);
    }

    #[test]
    fn the_launch_paths_read_from_isolooms_evidence() {
        let tiers = run_tiers();
        assert!(tiers.len() > 10, "isoloom exposes a tier for each launch path");
        let find = |target: &str, variant: &str| tiers.iter().find(|t| t.target == target && t.variant.as_deref() == Some(variant));
        // AWS has real-run evidence (proven in a real cloud): not Beta.
        assert_eq!(find("cloud-vm", "aws").map(|t| t.badge), Some(Badge::None), "aws cloud-vm: {:?}", find("cloud-vm", "aws"));
        // The validated-only clouds are Beta.
        assert_eq!(find("cloud-vm", "azure").map(|t| t.badge), Some(Badge::Beta));
        assert_eq!(find("cloud-vm", "gcp").map(|t| t.badge), Some(Badge::Beta));
    }
}
