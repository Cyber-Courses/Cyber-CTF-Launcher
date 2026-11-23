use super::{LabRef, Park, clear_parked, park_lab, park_refusal, parked_message, provision_lab, provision_target, refuse_cloud_park, resume_lab, stop_lab};
use crate::runtime::providers::testing::no_tools;
use crate::runtime::recorded::Recorded;
use crate::runtime::test_support::{record_parked, record_run};

use crate::runtime::Runtime;
use crate::runtime::test_support::status_with;
use crate::testing::test_app::TestApp;

#[tokio::test]
async fn a_lab_on_a_cloud_account_is_neither_parked_nor_provisioned_there() {
    let t = TestApp::new();
    t.lab("web", &[]);
    let host = t.host("az1", "azure", serde_json::json!({}));
    record_run(t.data(), "web", Some(&host), "cloud-docker", "failed");
    let tools = no_tools();
    let err = park_lab(&t.dirs(), "web", Runtime::Docker, Park::Pause, |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("Pausing a lab on a server or a cloud account"), "{err}");
    let err = resume_lab(&t.dirs(), "web", Runtime::Docker, |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("on a server or a cloud account isn't parked"), "{err}");
    let err = provision_lab(&t.dirs(), "web", Runtime::Vm, None, |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("Proxmox or a cloud"), "{err}");
    assert_eq!(tools.lines(), Vec::<String>::new());
    // Stopping it destroys it with Terraform there: with no module, nothing is left to destroy,
    // and the lab doesn't read as running there; the record stays, as nothing was removed.
    stop_lab(&t.dirs(), "web", Runtime::Docker, std::future::ready(()), |_| {}).await.unwrap();
    let dir = t.data().join("labs/web");
    assert!(!crate::runtime::status::status(&t.dirs(), &dir, "web", Runtime::Docker).await.unwrap().running);
    assert_eq!(Recorded::of(&t.dirs(), "web").await.unwrap().host(), Some("az1"));
    assert!(tools.lines().is_empty(), "{:?}", tools.lines());
}

#[tokio::test]
async fn the_policy_refuses_resuming_a_privileged_lab_before_anything_runs() {
    use crate::error::Code;
    use crate::platform::policy::{Key, Override};
    let t = TestApp::new();
    record_parked(t.data(), "priv", "docker", "pause");
    t.lab("priv", &[("isoloom.yml", crate::runtime::test_support::PRIVILEGED_LAB)]);
    let tools = no_tools();
    let _policy = Override::with(&[Key::RefusePrivilegedLabs]);
    let err = resume_lab(&t.dirs(), "priv", Runtime::Docker, |_| {}).await.unwrap_err();
    assert_eq!(err.code(), Code::TurnedOffByPolicy { setting: "RefusePrivilegedLabs".into() }, "{err}");
    assert_eq!(Recorded::of(&t.dirs(), "priv").await.unwrap().parked(), Some(Park::Pause));
    assert!(tools.lines().is_empty(), "{:?}", tools.lines());
}

#[tokio::test]
async fn a_parked_lab_whose_files_cant_be_made_again_is_left_recorded_and_says_why() {
    // Parked by a park, its files and its spec gone: the runner can't be asked to take it down,
    // and nothing here guesses it gone.
    let t = TestApp::new();
    let dir = t.lab("web", &[]);
    record_parked(t.data(), "web", "docker", "shutdown");
    let tools = no_tools();
    let mut lines = Vec::<String>::new();
    let lab = LabRef { dir: &dir, id: "web", runtime: Runtime::Docker };
    let err = clear_parked(&t.dirs(), lab, &mut |l| lines.push(l)).await.unwrap_err().to_string();
    assert!(err.starts_with("This lab's files are gone"), "{err}");
    assert!(lines[0].starts_with("A parked copy of an older version"), "{lines:?}");
    assert_eq!(Recorded::of(&t.dirs(), "web").await.unwrap().parked(), Some(Park::Shutdown));
    assert!(tools.lines().is_empty(), "{:?}", tools.lines());
}

#[tokio::test]
async fn a_parked_lab_that_couldnt_be_taken_down_stays_parked_and_says_why() {
    let t = TestApp::new();
    let dir = t.lab("web", &[]);
    record_parked(t.data(), "web", "docker", "pause");
    // The runner can't run the lab's project: the teardown fails.
    let compose = crate::runtime::lab::compose_file(&dir);
    std::fs::create_dir_all(compose.parent().unwrap()).unwrap();
    std::fs::write(&compose, "not a compose file").unwrap();
    let _tools = no_tools();
    let lab = LabRef { dir: &dir, id: "web", runtime: Runtime::Docker };
    assert!(clear_parked(&t.dirs(), lab, &mut |_| {}).await.is_err());
    assert!(Recorded::of(&t.dirs(), "web").await.unwrap().record().is_some(), "still recorded: its machines may still be there");
}

#[test]
fn provisioning_needs_a_running_lab_and_a_known_machine() {
    let err = provision_target(&status_with(false, &["dc01"]), None).unwrap_err().to_string();
    assert!(err.starts_with("Start (or resume) the lab first"), "{err}");
    let up = status_with(true, &["dc01", "ws01"]);
    assert_eq!(provision_target(&up, Some("nope")).unwrap_err().to_string(), "this lab has no machine `nope`");
    assert_eq!(provision_target(&up, Some("ws01")).unwrap(), "Re-running provisioning on ws01…");
    assert_eq!(provision_target(&up, None).unwrap(), "Re-running provisioning on every machine…");
}

#[test]
fn a_lab_is_parked_only_in_a_mode_its_page_is_offered() {
    use crate::runtime::lab::Target;
    let both = [Park::Pause, Park::Shutdown];
    // Containers on Docker pause (frozen) as well as shut down. Regression: a pause on QEMU went
    // to a `vagrant suspend` it refuses (the runner lists no pause for it).
    for mode in both {
        assert_eq!(park_refusal(mode, Some(Target::Docker), &both), None);
    }
    assert!(park_refusal(Park::Pause, Some(Target::Vagrant), &[Park::Shutdown]).unwrap().starts_with("QEMU can't pause"));
    assert_eq!(park_refusal(Park::Shutdown, Some(Target::Vagrant), &[Park::Shutdown]), None);
    assert_eq!(park_refusal(Park::Pause, Some(Target::Vagrant), &both), None);
    assert!(parked_message(Park::Pause).starts_with("Lab paused."));
    assert!(parked_message(Park::Shutdown).starts_with("Lab shut down."));
}

#[tokio::test]
async fn a_pause_on_qemu_is_refused_before_anything_runs() {
    let t = TestApp::new();
    let spec =
        "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, os: debian@12, networks: { lab: 10 } }\n";
    t.lab("t", &[("isoloom.yml", spec)]);
    crate::runtime::test_support::record_vms_on(t.data(), "t", "qemu");
    let tools = no_tools();
    let err = park_lab(&t.dirs(), "t", Runtime::Vm, Park::Pause, |_| {}).await.unwrap_err();
    assert!(matches!(err, crate::error::Error::Refused(_)) && err.to_string().starts_with("QEMU can't pause"), "{err}");
    assert!(tools.lines().is_empty(), "{:?}", tools.lines());
    assert_eq!(Recorded::of(&t.dirs(), "t").await.unwrap().parked(), None, "not recorded as paused");
}

#[test]
fn a_cloud_lab_is_never_parked() {
    assert!(refuse_cloud_park(Runtime::Cloud).unwrap_err().to_string().starts_with("A cloud lab can't be paused"));
    assert!(refuse_cloud_park(Runtime::Docker).is_ok());
    assert!(refuse_cloud_park(Runtime::Vm).is_ok());
}

#[tokio::test]
async fn stopping_a_cloud_lab_that_runs_nowhere_runs_nothing() {
    // A cloud-services lab has nothing on this machine: a stop with no account recorded runs
    // nothing.
    let t = TestApp::new();
    t.lab("goat", &[]);
    let tools = no_tools();
    stop_lab(&t.dirs(), "goat", Runtime::Cloud, std::future::ready(()), |_| {}).await.unwrap();
    assert!(tools.lines().is_empty(), "{:?}", tools.lines());
    // Parking or resuming it is refused on the runtime alone.
    let err = park_lab(&t.dirs(), "goat", Runtime::Cloud, Park::Pause, |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
    let err = resume_lab(&t.dirs(), "goat", Runtime::Cloud, |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
    assert_eq!(tools.lines(), Vec::<String>::new());
}

#[tokio::test]
async fn clearing_a_lab_that_is_not_parked_does_nothing() {
    let t = TestApp::new();
    let dir = t.lab("web-1", &[]);
    let tools = no_tools();
    let mut lines = Vec::<String>::new();
    clear_parked(&t.dirs(), LabRef { dir: &dir, id: "web-1", runtime: Runtime::Vm }, &mut |l| lines.push(l)).await.unwrap();
    assert_eq!(lines, [] as [String; 0]);
    assert_eq!(tools.lines(), [] as [String; 0]);
}

/// What the page may do on lab `t`, declared `runtime`, where it is located now, on `provider`
/// (its run parked as the runner says a run on it is): whether it pauses, shuts down, provisions
/// and repairs.
async fn controls_of(t: &TestApp, dir: &std::path::Path, runtime: Runtime, provider: Option<&str>) -> (bool, bool, bool, bool) {
    use crate::runtime::Control;
    let host = crate::runtime::host::locate(&t.dirs(), dir, "t", runtime).await.unwrap();
    let parks = match provider {
        Some("docker" | "virtualbox") => vec![Park::Pause, Park::Shutdown],
        Some("qemu") => vec![Park::Shutdown],
        _ => Vec::new(),
    };
    let c = super::controls(host.as_ref(), &parks);
    let has = |x| c.contains(&x);
    (has(Control::Pause), has(Control::Shutdown), has(Control::Provision), has(Control::Repair))
}

#[tokio::test]
async fn the_page_is_offered_what_the_lifecycle_accepts_where_the_lab_runs() {
    let t = TestApp::new();
    let _tools = no_tools();
    let spec =
        "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, os: debian@12, networks: { lab: 10 } }\n";
    let dir = t.lab("t", &[("isoloom.yml", spec)]);
    // Containers here: paused (frozen) or shut down, no provisioning; stopped containers
    // brought back as they are (regression, docs/contributing/testing/resilience.md M4: a Docker lab with a
    // container stopped outside the app was offered a reset only).
    assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("docker")).await, (true, true, false, true));
    // A VM lab here: both, provisioning and the repair; QEMU can't suspend.
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("virtualbox")).await, (true, true, true, true));
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("qemu")).await, (false, true, true, true));
    // A container lab run as one VM per machine is a VM lab here, repaired as one too.
    record_run(t.data(), "t", None, "vagrant", "ready");
    assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("virtualbox")).await, (true, true, true, true));
    record_run(t.data(), "t", None, "docker", "absent");
    // A cloud-services lab on no account: nothing runs.
    assert_eq!(controls_of(&t, &dir, Runtime::Cloud, None).await, (false, false, false, false));
    // In a cloud account, or on Proxmox: nothing parked or provisioned there.
    record_run(t.data(), "t", Some(&t.host("acct", "aws", serde_json::json!({ "useCliCreds": true }))), "cloud-vm", "ready");
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("aws")).await, (false, false, false, false));
    crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
    for host in ["pve", "esx"] {
        crate::runtime::server::secrets::set_secret(host, "pw").unwrap();
    }
    record_run(t.data(), "t", Some(&t.host("pve", "proxmox", serde_json::json!({}))), "proxmox", "ready");
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("proxmox")).await, (false, false, false, false));
    // On ESXi: provisioning a VM lab, through Vagrant there; never its containers' VM.
    record_run(t.data(), "t", Some(&t.host("esx", "vmware_esxi", serde_json::json!({}))), "vagrant", "ready");
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("vmware_esxi")).await, (false, false, true, false));
}
