//! The values a launch hands the lab: of what CyberBackend's `startLab` / `claimLaunch` answer
//! carries (`env`), only the evidence claim's two variables and the player's per-objective flags,
//! then the launcher's own extras. Either engine passes them on as the lab's inputs: the runner
//! delivers a value only to a slot the lab's manifest declares, and Isoloom turns a sensitive
//! input into a file in the machine (a Compose secret, the VM's guest delivery), never into its
//! environment.

use serde::Deserialize;

use crate::runtime;

/// One variable of a launch's `env`.
#[derive(Deserialize)]
pub(super) struct EnvVar {
    pub(super) name: String,
    pub(super) value: String,
}

/// The variables of the backend's evidence claim (a lab exchanges the token for its evidence).
const CLAIM: [&str; 2] = ["CTF_API_URL", "CTF_LAUNCH_TOKEN"];

/// The prefix of an objective's per-player value: CyberBackend names it `CTF_FLAG_<KEY>`, the
/// objective's key in upper snake case (`flagEnvName`: `main` is `CTF_FLAG_MAIN`, `ssrf-to-metadata`
/// is `CTF_FLAG_SSRF_TO_METADATA`).
const FLAG_PREFIX: &str = "CTF_FLAG_";

/// Whether `name` is one of the backend's per-objective flags (`CTF_FLAG_[A-Z0-9_]+`).
fn is_flag(name: &str) -> bool {
    name.strip_prefix(FLAG_PREFIX).is_some_and(|key| !key.is_empty() && key.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
}

/// The environment a launch runs with: the evidence claim's two variables and the player's flags
/// from the backend (nothing else it sends), then the attack-box image and the default-ports
/// switch when asked for.
pub(super) fn launch_env(env: Vec<EnvVar>, attackbox_image: Option<&str>, default_ports: bool) -> Vec<(String, String)> {
    env.into_iter()
        .filter(|v| CLAIM.contains(&v.name.as_str()) || is_flag(&v.name))
        .map(|v| (v.name, v.value))
        .chain(attackbox_image.map(|i| ("CYBERCTF_ATTACKBOX_IMAGE".to_string(), i.to_string())))
        .chain(default_ports.then(|| (runtime::PORTS_ENV.to_string(), "default".to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::testing::proptest_support::config;

    fn env(pairs: &[(&str, &str)]) -> Vec<EnvVar> {
        pairs.iter().map(|(n, v)| EnvVar { name: (*n).to_string(), value: (*v).to_string() }).collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(n, v)| ((*n).to_string(), (*v).to_string())).collect()
    }

    /// CyberBackend's `flagEnvName` (src/evidence.ts): an objective key's launch variable.
    fn backend_flag_name(key: &str) -> String {
        format!("{FLAG_PREFIX}{}", key.to_uppercase().replace('-', "_"))
    }

    #[test]
    fn a_launch_passes_the_evidence_claim_and_the_extras_asked_for() {
        let backend = env(&[("CTF_API_URL", "https://api"), ("PATH", "/evil"), ("CTF_LAUNCH_TOKEN", "t0k")]);
        assert_eq!(launch_env(backend, None, false), pairs(&[("CTF_API_URL", "https://api"), ("CTF_LAUNCH_TOKEN", "t0k")]));
        let extras = launch_env(env(&[]), Some("ghcr.io/x/box:1"), true);
        assert_eq!(extras, pairs(&[("CYBERCTF_ATTACKBOX_IMAGE", "ghcr.io/x/box:1"), (runtime::PORTS_ENV, "default")]));
    }

    /// Regression: only the claim's two variables were kept, so no player's flag ever reached a
    /// lab on either engine, and every lab ran with its development value.
    #[test]
    fn a_launch_passes_the_players_flags_for_each_objective() {
        let backend = env(&[
            ("CTF_API_URL", "https://api"),
            ("CTF_LAUNCH_TOKEN", "t0k"),
            ("CTF_FLAG_MAIN", "FLAG{p1-main}"),
            ("CTF_FLAG_SSRF_TO_METADATA", "FLAG{p1-ssrf}"),
            ("CTF_FLAG_CTF_KEY", "k3y"),
        ]);
        assert_eq!(
            launch_env(backend, None, false),
            pairs(&[
                ("CTF_API_URL", "https://api"),
                ("CTF_LAUNCH_TOKEN", "t0k"),
                ("CTF_FLAG_MAIN", "FLAG{p1-main}"),
                ("CTF_FLAG_SSRF_TO_METADATA", "FLAG{p1-ssrf}"),
                ("CTF_FLAG_CTF_KEY", "k3y"),
            ])
        );
    }

    #[test]
    fn a_name_that_only_looks_like_a_flag_is_dropped() {
        for name in [
            "CTF_FLAG_",
            "CTF_FLAGMAIN",
            "ctf_flag_main",
            "CTF_FLAG_main",
            "CTF_FLAG_SSRF-TO",
            "CTF_FLAG_A B",
            "CTF_FLAG_\u{c9}",
            "X_CTF_FLAG_MAIN",
            "LD_PRELOAD",
        ] {
            assert!(launch_env(env(&[(name, "v")]), None, false).is_empty(), "{name}");
        }
    }

    proptest! {
        #![proptest_config(config())]

        /// The mirror of CyberBackend's rule: every key a lab's objective can have (kebab-case, at
        /// most 64 characters) names a launch variable this passes, with its value unchanged.
        #[test]
        fn every_flag_the_backend_can_name_reaches_the_lab(key in "[a-z0-9]{1,12}(-[a-z0-9]{1,12}){0,4}", value in "\\PC{0,40}") {
            prop_assume!(key.len() <= 64);
            let name = backend_flag_name(&key);
            prop_assert_eq!(launch_env(env(&[(name.as_str(), value.as_str())]), None, false), pairs(&[(name.as_str(), value.as_str())]));
        }

        /// Nothing else the backend could send gets through.
        #[test]
        fn any_other_name_is_dropped(name in "[A-Za-z0-9_-]{0,24}") {
            prop_assume!(!CLAIM.contains(&name.as_str()) && !name.starts_with(FLAG_PREFIX));
            prop_assert!(launch_env(env(&[(name.as_str(), "v")]), None, false).is_empty());
        }
    }
}
