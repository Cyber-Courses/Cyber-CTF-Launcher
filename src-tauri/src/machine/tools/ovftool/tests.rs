use std::path::{Path, PathBuf};
use std::sync::Arc;

use isothrum_runner::fake::{FakeProcesses, Reply};

use super::*;
use crate::runtime::providers::testing::{Machine, host};
use crate::testing::test_app::TestApp;
use isothrum_machine::tools::ovftool::{Refusal, Source};

/// The OVF Tool as Broadcom's download unpacks under `root`: the program and its libraries.
fn download(root: &Path) -> PathBuf {
    let exe = root.join(ovftool::exe(isothrum_runner::this_host()));
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(root.join("lib").join("libvmomi.dylib"), b"lib").unwrap();
    std::fs::write(root.join("vmomi.dll"), b"lib").unwrap();
    std::fs::write(root.join("libvmomi.so.1"), b"lib").unwrap();
    std::fs::write(&exe, b"ovftool").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    root.to_path_buf()
}

/// This machine's tools, under `root`: `codesign` answering for VMware's own signature, the OVF
/// Tool its version, the rest succeeding.
fn vmware_tools(root: &Path) -> Machine {
    let tools = FakeProcesses::model(|call| match call.program.as_str() {
        "/usr/bin/codesign" if call.args.first().map(String::as_str) == Some("-d") => {
            Reply::ok(&format!("designated => identifier ovftool and anchor apple generic and certificate leaf[subject.OU] = {}\n", ovftool::VMWARE_TEAM))
        }
        p if Path::new(p).file_stem().is_some_and(|s| s == "ovftool") => Reply::ok("VMware ovftool 5.1.0 (build-24936374)\n"),
        _ => Reply::ok(""),
    });
    Machine::set(Arc::new(tools), host(root))
}

/// A copy placed into the app's folder is reported there by the next check.
#[tokio::test]
async fn a_placed_copy_is_reported_by_the_next_check() {
    let t = TestApp::new();
    let root = tempfile::tempdir().unwrap();
    let picked = download(&root.path().join("download"));
    let _machine = vmware_tools(root.path());
    let outcome = place(&t.dirs(), &picked).await.unwrap();
    let placed = managed_dir(t.data()).join(ovftool::exe(isothrum_runner::this_host()));
    assert!(matches!(&outcome, Outcome::Placed { path, .. } if Path::new(path) == placed), "{outcome:?}");
    let (tool, location) = probe(&providers::machine(), Some(t.data())).await;
    assert_eq!(tool.version.as_deref(), Some("VMware ovftool 5.1.0 (build-24936374)"));
    let location = location.unwrap();
    assert_eq!((location.source, PathBuf::from(location.path)), (Source::Managed, placed));
}

#[tokio::test]
async fn placing_is_refused_when_the_policy_turns_tool_installs_off() {
    let _policy = crate::platform::policy::Override::with(&[Key::DisableToolInstalls]);
    let t = TestApp::new();
    let root = tempfile::tempdir().unwrap();
    let picked = download(&root.path().join("download"));
    let _machine = vmware_tools(root.path());
    let err = place(&t.dirs(), &picked).await.unwrap_err();
    assert!(matches!(err.code(), crate::error::Code::TurnedOffByPolicy { .. }), "{err}");
    assert!(!t.data().join("tools").exists());
}

#[test]
fn the_outcomes_reach_the_webview_tagged() {
    let placed = serde_json::to_value(Outcome::Placed { path: "/x/ovftool".into(), needs_rosetta: true }).unwrap();
    assert_eq!(placed, serde_json::json!({ "outcome": "placed", "path": "/x/ovftool", "needsRosetta": true }));
    let refused = serde_json::to_value(Outcome::Refused { reason: Refusal::NotVmware, detail: Some("ABCDE12345".into()) }).unwrap();
    assert_eq!(refused, serde_json::json!({ "outcome": "refused", "reason": "notVmware", "detail": "ABCDE12345" }));
}
