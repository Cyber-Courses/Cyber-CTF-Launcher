//! The launcher's machine check: isothrum-machine's report (tested there, tool by tool) in the
//! shape the webview reads, with the OVF Tool and the setup's options.

use std::sync::Arc;

use isothrum_runner::fake::{FakeProcesses, Reply};

use crate::runtime::providers::testing::{Machine, host};

/// The check reports the OVF Tool the setup placed in the app's folder, and where it is.
#[tokio::test]
async fn the_check_says_where_the_ovf_tool_was_found() {
    use crate::machine::tools::ovftool::managed_dir;
    use isothrum_machine::tools::ovftool::Source;
    let root = tempfile::tempdir().unwrap();
    let tools = FakeProcesses::model(|call| {
        if std::path::Path::new(&call.program).file_stem().is_some_and(|s| s == "ovftool") {
            Reply::ok("VMware ovftool 5.1.0 (build-24936374)\n")
        } else {
            Reply::missing()
        }
    });
    let _machine = Machine::set(Arc::new(tools), host(root.path()));
    let t = crate::testing::test_app::TestApp::new();
    let report = super::system_check(t.dirs()).await;
    assert!(report.ovftool_location.is_none() && !report.ovftool.installed, "none placed yet: none found, whatever this computer has");
    let exe = isothrum_machine::tools::ovftool::exe(isothrum_runner::this_host());
    std::fs::create_dir_all(managed_dir(t.data())).unwrap();
    std::fs::write(managed_dir(t.data()).join(exe), "").unwrap();
    let report = super::system_check(t.dirs()).await;
    assert_eq!(report.ovftool.version.as_deref(), Some("VMware ovftool 5.1.0 (build-24936374)"));
    let location = report.ovftool_location.unwrap();
    assert_eq!((location.source, std::path::PathBuf::from(&location.path)), (Source::Managed, managed_dir(t.data()).join(exe)));
    let json = serde_json::to_value(&location).unwrap();
    assert!(json.get("needsRosetta").is_some() && json.get("source").is_some(), "{json}");
}

/// The machine check's report with `tools` as this machine's.
async fn checked(tools: FakeProcesses) -> super::SystemReport {
    let root = tempfile::tempdir().unwrap();
    let _machine = Machine::set(Arc::new(tools), host(root.path()));
    let t = crate::testing::test_app::TestApp::new();
    super::system_check(t.dirs()).await
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
    let report = checked(tools).await;
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
    let report = checked(tools).await;
    assert!(report.docker_denied && !report.docker_running && report.docker_denied_hint.is_some());
}

/// The webview's `Os` and `Arch` are the ids the report writes: isothrum-machine's for an OS, and
/// Rust's name for a CPU.
#[test]
fn the_reports_os_and_cpu_types_are_what_it_writes() {
    use ts_rs::TS;
    let cfg = ts_rs::Config::default();
    let oses: Vec<String> = isothrum_machine::host::ALL_OSES.iter().map(|os| serde_json::to_string(os).unwrap()).collect();
    assert_eq!(super::OsId::inline(&cfg), oses.join(" | "));
    let cpus: Vec<String> = isothrum_machine::host::ALL_ARCHES.iter().map(|arch| format!("\"{}\"", crate::platform::cpu_name(*arch))).collect();
    assert_eq!(super::CpuName::inline(&cfg), cpus.join(" | "));
}
