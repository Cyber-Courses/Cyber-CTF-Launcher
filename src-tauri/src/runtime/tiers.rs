//! How far each launch path has been proven, from Isoloom's tier evidence (`isoloom-targets`'s
//! bundled `tiers.json`). The launcher shows a badge off the real-run **evidence**, not the tier
//! number: a path with no real run yet (validated only) is Beta, whatever its tier; a path real
//! runs have passed (AWS, Docker, the proven VM providers…) carries no badge even while tier 1
//! waits for the other host OSes. This updates itself as the evidence does — no hardcoded list.

use isoloom_targets::tiers::{self, Tier};

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
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
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

/// The badge an evidence line earns: Experimental for an experimental target, Beta when it is
/// validated only (no real run), else None (real runs have passed).
fn badge_of(tier: Tier, evidence: &str) -> Badge {
    if tier == Tier::Three || evidence.starts_with("experimental") {
        Badge::Experimental
    } else if evidence.starts_with("validated only") {
        Badge::Beta
    } else {
        Badge::None
    }
}

/// Every launch path's maturity, from Isoloom's tier evidence. Empty when the evidence can't be
/// read (the launcher then shows no badges rather than guessing).
pub fn run_tiers() -> Vec<RunTier> {
    let Ok(entries) = tiers::tiers() else { return Vec::new() };
    entries
        .into_iter()
        .map(|e| {
            let evidence = e.evidence();
            RunTier { target: e.target.id().to_owned(), variant: e.variant.map(str::to_owned), badge: badge_of(e.tier(), &evidence), evidence }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_not_the_tier_number_decides_the_badge() {
        // A validated-only cloud is Beta even at tier 2; a cloud real runs have passed is not,
        // even while tier 1 waits for the other host OSes.
        assert_eq!(badge_of(Tier::Two, "validated only; tier 1 pending: linux, macos, windows"), Badge::Beta);
        assert_eq!(badge_of(Tier::Two, "real runs: macos (mac, 2026-10-17); tier 1 pending: linux, windows"), Badge::None);
        assert_eq!(badge_of(Tier::One, "real runs: linux (jarvis, 2026-10-15), macos (mac, 2026-10-15), windows (karen, 2026-10-15)"), Badge::None);
        assert_eq!(badge_of(Tier::Three, "experimental"), Badge::Experimental);
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
