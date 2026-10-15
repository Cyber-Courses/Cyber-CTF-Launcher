//! End-to-end runs of the cloud paths against stand-in `terraform`, `gcloud` and `az` scripts put
//! first on PATH: real processes through the real `exec` code (spawn, stream, timeouts), no
//! cloud. They change the process's PATH, so they are opt-in and run alone:
//!
//!   cargo test fake_clis -- --ignored --test-threads=1

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::testing::test_app::TestApp;

/// A folder of stand-in tools, put first on PATH while it lives.
struct FakeBin {
    dir: PathBuf,
    path: std::ffi::OsString,
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
        FakeBin { dir, path }
    }

    /// A tool `name` running the shell `script`.
    fn tool(&self, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let file = self.dir.join(name);
        std::fs::write(&file, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
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

/// A lab `id` on an AWS account (CLI credentials) whose Isoloom module exists, deployed in
/// `region` and past its stop time. Returns the lab folder and the state folder.
fn deployed_on_aws(t: &TestApp, id: &str, region: &str) -> (PathBuf, PathBuf) {
    let dir = t.lab(id, &[(".isoloom/cloud-docker/aws/main.tf", "")]);
    let host = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true, "host": "us-east-1" }));
    crate::runtime::server::mark_lab(&dir, Some(&host)).unwrap();
    let state = t.data().join("deployments").join(id).join("aws");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[{"type":"aws_instance"}]}"#).unwrap();
    std::fs::write(state.join("run.json"), format!(r#"{{"expires_at":1,"region":"{region}","env":{{"AWS_REGION":"{region}"}}}}"#)).unwrap();
    std::fs::write(state.join("host"), &host).unwrap();
    (dir, state)
}

/// A fake terraform that, on `destroy`, writes the region it was given to `seen` and runs `then`.
fn terraform(bin: &FakeBin, then: &str) -> PathBuf {
    let seen = bin.file("seen");
    bin.tool(
        "terraform",
        &format!(
            r#"case "$1" in version) echo "Terraform v1.9.8";; init) echo initialized;; destroy) echo "$TF_VAR_region $AWS_REGION" > '{}'; {then};; esac"#,
            seen.display()
        ),
    );
    seen
}

#[tokio::test]
#[ignore = "changes PATH: run alone, see the module"]
async fn fake_clis_the_reaper_destroys_where_the_lab_was_created_and_is_not_held_by_a_leftover() {
    let bin = FakeBin::new();
    // The destroy leaves a process holding its output open (a provider plugin, an ssh master).
    let seen = terraform(&bin, "sleep 60 & echo destroyed");
    let t = TestApp::new();
    let (dir, state) = deployed_on_aws(&t, "e2e-reap", "eu-west-3");
    let started = Instant::now();
    crate::runtime::reap_expired_labs(t.handle()).await;
    assert!(started.elapsed() < Duration::from_secs(20), "held by the leftover: {:?}", started.elapsed());
    assert_eq!(std::fs::read_to_string(seen).unwrap().trim(), "eu-west-3 eu-west-3", "the account now says us-east-1");
    assert!(!state.join("terraform.tfstate").exists() && crate::runtime::server::marked_host(&dir).is_none());
}

#[tokio::test]
#[ignore = "changes PATH: run alone, see the module"]
async fn fake_clis_a_failing_auto_stop_is_recorded_until_the_credentials_work() {
    let bin = FakeBin::new();
    terraform(&bin, "echo 'Error: ExpiredToken: The security token included in the request is expired' >&2; exit 1");
    let t = TestApp::new();
    let (_, state) = deployed_on_aws(&t, "e2e-reap-fail", "eu-west-3");
    crate::runtime::reap_expired_labs(t.handle()).await;
    let shown = crate::runtime::reaper::last_failure(t.handle(), "e2e-reap-fail").expect("recorded").message;
    assert!(shown.contains("ExpiredToken"), "{shown}");
    assert!(state.join("terraform.tfstate").is_file());
    terraform(&bin, "echo destroyed");
    crate::runtime::reap_expired_labs(t.handle()).await;
    assert_eq!(crate::runtime::reaper::last_failure(t.handle(), "e2e-reap-fail"), None);
    assert!(!state.join("terraform.tfstate").exists());
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
    let err = crate::runtime::server::gcp::labs_project(t.handle(), &id).await.unwrap_err().to_string();
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
    let result = serde_json::to_value(crate::runtime::server::server_test(t.handle().clone(), id).await.unwrap()).unwrap();
    assert!(started.elapsed() < crate::platform::exec::CLI_TIMEOUT + Duration::from_secs(15), "{:?}", started.elapsed());
    assert_eq!(result["ok"], false);
    assert!(result["checks"].to_string().contains("timed out after 60s"), "{result}");
}
