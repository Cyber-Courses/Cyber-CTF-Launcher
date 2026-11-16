//! Reading Docker here: the address blocks its networks hold, and whether it answers at all.
//! Where a running lab is reachable comes from the runner's status (`isothrum::web`).

use super::compose;

mod networks;

pub use networks::subnets_in_use;
#[cfg(test)]
use networks::{ipv4_subnets, networks_of_others};

/// Whether this machine's Docker answers: its daemon names its OS to this account
/// (isothrum-machine's [`daemon_os`](isothrum_machine::docker::daemon_os)). A Docker that isn't
/// installed or isn't running doesn't.
pub async fn answers() -> bool {
    isothrum_machine::docker::daemon_os(&crate::runtime::providers::machine_check()).await.is_ok()
}

#[cfg(test)]
mod tests;
