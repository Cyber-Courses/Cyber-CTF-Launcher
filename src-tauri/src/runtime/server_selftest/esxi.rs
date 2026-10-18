//! The ESXi self-test, connection-only: reach the host's SSH port, which proves the address and
//! that it is up. Labs run on ESXi through the Isothrum runner (Vagrant from the runner) now, so
//! the launcher no longer stands up a throwaway VM of its own to test a host.

use std::path::Path;
use std::time::Duration;

use super::{Ran, Reporter, wait_tcp};
use crate::error::Error;
use crate::runtime::server;

/// Nothing is created connection-only, so there is never a VM left to remove.
pub(super) fn leftover(_r: &Reporter, _conn: &server::Connection, _work: &Path) {}

pub(super) async fn run(r: &Reporter, conn: &server::Connection, _work: &Path) -> Ran {
    let host = conn.env.iter().find(|(k, _)| k == "CYBERCTF_ESXI_HOSTNAME").map(|(_, v)| v.clone());
    let result = r
        .step("ssh", "Reach the ESXi host over SSH", async {
            let Some(host) = host else {
                return Err(Error::Invalid("the host's address isn't set".into()));
            };
            wait_tcp(&host, 22, Duration::from_secs(30)).await?;
            Ok(((), Some(format!("{host}:22 open"))))
        })
        .await;
    // Connection-only: nothing was created, so there is nothing to clean up.
    Ran { result, cleaned: true }
}
