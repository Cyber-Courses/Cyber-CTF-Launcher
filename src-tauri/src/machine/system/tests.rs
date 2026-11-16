//! The launcher's machine check: isothrum-machine's report (tested there, tool by tool) in the
//! shape the webview reads, with the OVF Tool and the setup's options.

use std::sync::Arc;

use isothrum_runner::fake::{FakeProcesses, Reply};

use crate::runtime::providers::testing::{Machine, host};

/// The check reports the OVF Tool the setup placed in the app's folder, and where it is.
#[tokio::test]
async fn the_check_says_where_the_ovf_tool_was_found() {
    use crate::machine::tools::ovftool::{Source, managed_dir};
    use crate::platform::exec::fake::{Tools, ok};
    let t = crate::testing::test_app::TestApp::new();
    let _tools = Tools::new(|program, _| if program == "ovftool" { ok("VMware ovftool 5.1.0 (build-24936374)\n") } else { None });
    let report = super::system_check(t.dirs()).await;
    let fusion = std::path::Path::new("/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool/ovftool").is_file();
    if report.ovftool_location.is_none() {
        assert!(!report.ovftool.installed, "nothing found, nothing installed");
    }
    let exe = if cfg!(windows) { "ovftool.exe" } else { "ovftool" };
    std::fs::create_dir_all(managed_dir(t.data())).unwrap();
    std::fs::write(managed_dir(t.data()).join(exe), "").unwrap();
    let report = super::system_check(t.dirs()).await;
    assert_eq!(report.ovftool.version.as_deref(), Some("VMware ovftool 5.1.0 (build-24936374)"));
    let location = report.ovftool_location.unwrap();
    if !fusion && !cfg!(windows) {
        assert_eq!((location.source, std::path::PathBuf::from(&location.path)), (Source::Managed, managed_dir(t.data()).join(exe)));
    }
    let json = serde_json::to_value(&location).unwrap();
    assert!(json.get("needsRosetta").is_some() && json.get("source").is_some(), "{json}");
}

/// Docker down and Vagrant missing: the report says so for them, and is made all the same.
#[tokio::test]
async fn the_report_carries_what_isothrum_machine_found() {
    let tools = FakeProcesses::model(|call| match (call.program.as_str(), call.args.first().map(String::as_str)) {
        ("docker", Some("--version")) => Reply::ok("Docker version 28.4.0, build d8eb465"),
        ("docker", _) => Reply::fail(1, "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"),
        ("terraform", _) => Reply::ok("Terraform v1.9.8\n"),
        _ => Reply::missing(),
    });
    let root = tempfile::tempdir().unwrap();
    let _machine = Machine::set(Arc::new(tools), host(root.path()));
    let t = crate::testing::test_app::TestApp::new();
    let report = super::system_check(t.dirs()).await;
    assert_eq!(report.docker.version.as_deref(), Some("Docker version 28.4.0, build d8eb465"));
    assert!(!report.docker_running && !report.docker_denied && report.docker_denied_hint.is_none() && report.docker_engine.is_none());
    assert!(!report.vagrant.installed && !report.cloud_clis.azure.installed && report.ready_hypervisors.is_empty());
    assert_eq!(report.terraform.version.as_deref(), Some("Terraform v1.9.8"));
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["pkgManager"]["installed"], false);
    assert_eq!(json["docker"], serde_json::json!({ "installed": true, "version": "Docker version 28.4.0, build d8eb465" }));
}

/// A daemon that refuses this account: denied, with the coded hint the webview says.
#[tokio::test]
async fn a_daemon_that_refuses_this_account_comes_with_its_hint() {
    let tools = FakeProcesses::model(|call| match (call.program.as_str(), call.args.first().map(String::as_str)) {
        ("docker", Some("info")) => Reply::fail(1, "permission denied while trying to connect to the docker API at unix:///var/run/docker.sock"),
        ("docker", Some("--version")) => Reply::ok("Docker version 26.1.5"),
        _ => Reply::missing(),
    });
    let root = tempfile::tempdir().unwrap();
    let _machine = Machine::set(Arc::new(tools), host(root.path()));
    let t = crate::testing::test_app::TestApp::new();
    let report = super::system_check(t.dirs()).await;
    assert!(report.docker_denied && !report.docker_running && report.docker_denied_hint.is_some());
}
