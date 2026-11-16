//! Reading Docker here: the address blocks its networks hold, and whether it answers at all.
//! Where a running lab is reachable comes from the runner's status (`isothrum::web`).

use super::compose;
use crate::platform::exec::run_read;

mod networks;

pub use networks::subnets_in_use;
#[cfg(test)]
use networks::{ipv4_subnets, networks_of_others};

/// Whether this machine's Docker answers: `docker version` reaches its daemon (for this account),
/// which names its OS. A Docker that isn't installed or isn't running doesn't.
pub async fn answers() -> bool {
    run_read("docker", &["version", "--format", "{{.Server.Os}}"], None).await.is_ok_and(|os| !os.trim().is_empty())
}

#[cfg(test)]
mod tests;
