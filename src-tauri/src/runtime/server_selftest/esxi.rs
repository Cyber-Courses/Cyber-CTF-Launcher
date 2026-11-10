//! The ESXi self-test, connection-only: reach the host's SSH port, which proves the address and
//! that it is up. Labs run on ESXi through the Isothrum runner (Vagrant from the runner) now, so
//! the launcher no longer stands up a throwaway VM of its own to test a host.

use std::path::Path;
use std::time::Duration;

use super::{Ran, Reporter, wait_tcp};
use crate::error::Error;
use crate::runtime::server;

/// The steps the test reports, in order (the frontend lists the same: [`super::PLANS`]).
#[cfg(test)]
pub(super) const PLAN: [&str; 2] = [super::CONNECT, SSH];

/// Its one step of its own: the host's SSH port answers.
const SSH: &str = "ssh";

pub(super) async fn run(r: &Reporter, conn: &server::Connection, _work: &Path) -> Ran {
    let host = conn.env.iter().find(|(k, _)| k == "CYBERCTF_ESXI_HOSTNAME").map(|(_, v)| v.clone());
    let result = r
        .step(SSH, "Reach the ESXi host over SSH", async {
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn the_test_reports_the_steps_of_its_plan() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let r = Reporter::new(move |e: super::super::Event| sink.lock().unwrap().push(e.step));
        let conn = server::Connection {
            id: "esxi1".into(),
            provider: crate::runtime::providers::Provider::VmwareEsxi,
            name: "ESXi".into(),
            env: Vec::new(),
            tf_vars: Vec::new(),
            tf_env: Vec::new(),
            region: None,
            auto_stop_hours: None,
        };
        // No address: its one step fails, as the plan's step after the connection.
        assert!(run(&r, &conn, Path::new("/nonexistent")).await.result.is_err());
        let mut steps = seen.lock().unwrap().clone();
        steps.dedup();
        assert_eq!(steps, PLAN[1..]);
    }
}
