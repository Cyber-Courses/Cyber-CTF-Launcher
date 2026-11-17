//! End-to-end runs of the cloud paths against stand-in `terraform`, `gcloud` and `az` scripts put
//! first on PATH: real processes through the real `exec` code (spawn, stream, timeouts), no
//! cloud. They change the process's PATH, so they are opt-in and run alone:
//!
//!   cargo test `fake_clis` -- --ignored --test-threads=1
#![expect(
    unsafe_code,
    reason = "set_var, to put the stand-in tools first on PATH, is unsafe in edition 2024; these tests run alone (--test-threads=1, see above) so nothing else reads the environment meanwhile"
)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::testing::test_app::TestApp;

/// A folder of stand-in tools, put first on PATH while it lives, which the cloud CLIs are run
/// from: isothrum-machine asks them through the runner's real processes on this test's thread.
struct FakeBin {
    dir: PathBuf,
    path: std::ffi::OsString,
    _machine: crate::runtime::providers::testing::Machine,
}

impl FakeBin {
    fn new() -> FakeBin {
        let dir = std::env::temp_dir().join(format!("cyberctf-fakebin-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let joined = std::env::join_paths(std::iter::once(dir.clone()).chain(std::env::split_paths(&path))).unwrap();
        // SAFETY: these tests run alone (`--test-threads=1`, see the module): nothing else reads
        // the environment meanwhile.
        unsafe { std::env::set_var("PATH", joined) };
        let machine = crate::runtime::providers::testing::Machine::set(
            std::sync::Arc::new(isothrum_runner::SystemProcesses::default()),
            crate::runtime::providers::testing::host(&dir),
        );
        FakeBin { dir, path, _machine: machine }
    }

    /// A tool `name` running the shell `script` (written by a child process: see
    /// [`crate::testing::write_executable`]).
    fn tool(&self, name: &str, script: &str) {
        crate::testing::write_executable(self.dir.join(name), format!("#!/bin/sh\n{script}\n"));
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for FakeBin {
    fn drop(&mut self) {
        // SAFETY: as in `new`.
        unsafe { std::env::set_var("PATH", &self.path) };
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
#[ignore = "changes PATH: run alone, see the module"]
async fn fake_clis_an_unreachable_gcloud_never_replaces_the_labs_project() {
    let bin = FakeBin::new();
    let created = bin.file("created");
    bin.tool(
        "gcloud",
        &format!(r#"case "$2" in describe|list) echo "ERROR: (gcloud.projects.$2) There was a problem refreshing your current auth tokens: network unreachable" >&2; exit 1;; create) touch '{}';; esac"#, created.display()),
    );
    let t = TestApp::new();
    let id = t.host("gcp1", "gcp", serde_json::json!({ "username": "0X0X0X-0X0X0X-0X0X0X", "gcpProject": "cyberctf-labs-aaaaaa" }));
    let err = crate::runtime::server::gcp::labs_project(&t.dirs(), &id).await.unwrap_err().to_string();
    assert!(err.contains("network unreachable"), "{err}");
    assert!(!created.exists(), "no new project");
}

#[tokio::test]
#[ignore = "changes PATH: run alone, see the module (about a minute: the CLI time limit)"]
async fn fake_clis_a_hung_cloud_cli_ends_the_test_in_its_time_limit() {
    let bin = FakeBin::new();
    // An `az` waiting for a re-authentication nobody gives.
    bin.tool("az", "sleep 600");
    let t = TestApp::new();
    let id = t.host("az1", "azure", serde_json::json!({ "username": "00000000-0000-0000-0000-000000000000" }));
    let started = Instant::now();
    let result = serde_json::to_value(crate::runtime::server::connection_test::test_connection(&t.dirs(), &id).await.unwrap()).unwrap();
    assert!(started.elapsed() < isothrum_machine::cloud::CLI_TIMEOUT + Duration::from_secs(15), "{:?}", started.elapsed());
    assert_eq!(result["ok"], false);
    assert!(result["checks"].to_string().contains("timed out after 60s"), "{result}");
}
