//! The values the CyberCTF platform promises every lab it runs, given to each of the run's
//! operations whether or not a launch gave them.

use isothrum_runner::{RunOptions, SecretValue};

/// The values the CyberCTF platform gives every lab it runs, whatever the lab: its evidence claim,
/// the API a lab claims the player's evidence from and the launch's single-use token
/// (CyberBackend's `startLab` and `claimLaunch` answer both). A lab declares them as its inputs,
/// the token a sensitive one, delivered as a file.
pub(crate) const CLAIM: [&str; 2] = ["CTF_API_URL", "CTF_LAUNCH_TOKEN"];

/// `opts` with each of the [`CLAIM`]'s values, empty when `opts` has none: a lab imported from a
/// folder has no launch, and no operation after a start has its values (a resume, the checks: the
/// token is single-use and never kept). Empty is what a lab reads as no claim, and it then uses
/// its development evidence. Unset isn't: a sensitive input is a Compose secret read from its
/// variable, and Compose refuses to create a container while that variable is unset.
pub(super) fn with_claim(mut opts: RunOptions) -> RunOptions {
    for name in CLAIM {
        if opts.secrets.get(name).is_none() && !opts.env.contains_key(name) {
            opts.secrets.insert(name, SecretValue::new(String::new()));
        }
    }
    opts
}
