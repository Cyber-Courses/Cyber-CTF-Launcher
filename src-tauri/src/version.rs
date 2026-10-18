//! The engine versions this launcher bundles, taken from its pinned Isoloom and Isothrum
//! dependencies at build time (see build.rs, which reads them from Cargo.lock). The lab catalogue
//! query passes them to CyberBackend so only labs this launcher can generate (Isoloom) and run
//! (Isothrum) are shown; an older launcher correctly hides labs published for a newer engine.

/// The bundled Isoloom version, the version of the pinned `isoloom-*` crates.
pub(crate) const ISOLOOM: &str = env!("CYBERCTF_ISOLOOM_VERSION");

/// The bundled Isothrum version, the version of the pinned `isothrum-runner` crate.
pub(crate) const ISOTHRUM: &str = env!("CYBERCTF_ISOTHRUM_VERSION");

#[cfg(test)]
mod tests {
    use super::{ISOLOOM, ISOTHRUM};

    #[test]
    fn the_bundled_engine_versions_are_non_empty_dotted_versions() {
        for v in [ISOLOOM, ISOTHRUM] {
            assert!(!v.is_empty(), "a bundled engine version is empty");
            assert!(v.split('.').count() >= 3, "not a semver: {v}");
            assert!(v.chars().next().is_some_and(|c| c.is_ascii_digit()), "not a version: {v}");
        }
    }
}
