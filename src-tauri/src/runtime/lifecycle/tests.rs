use super::{LabRef, Park, clear_parked, park_lab, park_refusal, parked_message, provision_lab, provision_target, refuse_cloud_park, resume_lab, stop_lab};
use crate::platform::exec::fake::Tools;
use crate::runtime::paths::{mark_parked, parked};

use crate::runtime::test_support::status_with;
use crate::runtime::{Runtime, server};
use crate::testing::test_app::TestApp;

#[tokio::test]
async fn a_lab_on_a_cloud_account_is_neither_parked_nor_provisioned_there() {
    let t = TestApp::new();
    let dir = t.lab("web", &[]);
    let host = t.host("az1", "azure", serde_json::json!({}));
    server::mark_lab(&dir, Some(&host)).unwrap();
    let tools = Tools::strict_rules(&[]);
    let err = park_lab(&t.dirs(), "web", Runtime::Docker, Park::Pause, |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("Pausing a lab on a server or a cloud account"), "{err}");
    let err = resume_lab(&t.dirs(), "web", Runtime::Docker, |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("on a server or a cloud account isn't parked"), "{err}");
    let err = provision_lab(&t.dirs(), "web", Runtime::Vm, None, |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("Proxmox or a cloud"), "{err}");
    assert_eq!(tools.calls(), Vec::<String>::new());
    // Stopping it destroys it with Terraform there: with no state, nothing is left to destroy,
    // and the lab no longer reads as running there.
    stop_lab(&t.dirs(), "web", Runtime::Docker, std::future::ready(()), |_| {}).await.unwrap();
    assert!(!dir.join(server::HOST_MARKER).exists());
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

#[tokio::test]
async fn a_stop_says_what_it_cant_remove_of_a_deployment_the_runner_hasnt_taken_over() {
    // Regression: the Stop went through, the deployment of the launcher's own Terraform path
    // the runner couldn't take stayed up (nothing runs Terraform for it any more), and the
    // player had been told Stop would remove it.
    let t = TestApp::new();
    let dir = t.lab("web", &[]);
    let host = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true, "host": "us-east-1" }));
    server::mark_lab(&dir, Some(&host)).unwrap();
    let state = t.data().join("deployments/web/aws");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[{"type":"aws_instance"}]}"#).unwrap();
    std::fs::write(state.join("host"), &host).unwrap();
    let tools = Tools::strict_rules(&[]);
    let err = stop_lab(&t.dirs(), "web", Runtime::Docker, std::future::ready(()), |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("the launcher can't remove") && err.contains("delete its resources in your AWS console"), "{err}");
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    // Once the runner took it over (no resources left in the old state), the Stop goes through.
    std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[]}"#).unwrap();
    stop_lab(&t.dirs(), "web", Runtime::Docker, std::future::ready(()), |_| {}).await.unwrap();
}

#[tokio::test]
async fn the_policy_refuses_resuming_a_privileged_lab_before_anything_runs() {
    use crate::error::Code;
    use crate::platform::policy::{Key, Override};
    let t = TestApp::new();
    let dir = t.lab("priv", &[("isoloom.yml", crate::runtime::test_support::PRIVILEGED_LAB)]);
    mark_parked(&dir, Some(Park::Pause)).unwrap();
    let tools = Tools::strict_rules(&[]);
    let _policy = Override::with(&[Key::RefusePrivilegedLabs]);
    let err = resume_lab(&t.dirs(), "priv", Runtime::Docker, |_| {}).await.unwrap_err();
    assert_eq!(err.code(), Code::TurnedOffByPolicy { setting: "RefusePrivilegedLabs".into() }, "{err}");
    assert_eq!(parked(&dir), Some(Park::Pause));
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

#[tokio::test]
async fn a_parked_lab_is_taken_down_through_the_runner_before_its_folder_is_replaced() {
    let t = TestApp::new();
    let dir = t.lab("web", &[]);
    mark_parked(&dir, Some(Park::Shutdown)).unwrap();
    crate::runtime::paths::mark_engine(&dir, Some("orbstack")).unwrap();
    let tools = Tools::strict_rules(&[]);
    let mut lines = Vec::<String>::new();
    let lab = LabRef { dir: &dir, id: "web", runtime: Runtime::Docker };
    clear_parked(&t.dirs(), lab, &mut |l| lines.push(l)).await.unwrap();
    // No files of a run left: nothing to remove, and its marks go.
    assert_eq!(parked(&dir), None);
    assert_eq!(crate::runtime::paths::engine(&dir), None);
    assert!(lines[0].starts_with("A parked copy of an older version"), "{lines:?}");
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

#[tokio::test]
async fn a_parked_lab_that_couldnt_be_taken_down_stays_parked_and_says_why() {
    let t = TestApp::new();
    let dir = t.lab("web", &[]);
    mark_parked(&dir, Some(Park::Pause)).unwrap();
    // The runner can't read the run's record: the teardown fails.
    let manifest = crate::runtime::lab::docker_manifest(&dir);
    std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    std::fs::write(&manifest, "not a manifest").unwrap();
    let _tools = Tools::strict_rules(&[]);
    let lab = LabRef { dir: &dir, id: "web", runtime: Runtime::Docker };
    assert!(clear_parked(&t.dirs(), lab, &mut |_| {}).await.is_err());
    assert_eq!(parked(&dir), Some(Park::Pause), "still parked: its machines may still be there");
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
    // A container lab an earlier version runs inside a helper VM is only stopped.
    for mode in [Park::Pause, Park::Shutdown] {
        assert_eq!(park_refusal(mode, None, Some("virtualbox")), Some(super::LEFT_IN_A_HELPER_VM));
    }
    // Regression: a pause of containers on Docker was carried out as a shut down and
    // announced as a pause, and a pause on QEMU went to a `vagrant suspend` it refuses.
    assert!(park_refusal(Park::Pause, Some(Target::Docker), Some("docker")).unwrap().contains("shut it down instead"));
    assert_eq!(park_refusal(Park::Shutdown, Some(Target::Docker), Some("docker")), None);
    assert!(park_refusal(Park::Pause, Some(Target::Vagrant), Some("qemu")).unwrap().starts_with("QEMU can't pause"));
    assert_eq!(park_refusal(Park::Shutdown, Some(Target::Vagrant), Some("qemu")), None);
    assert_eq!(park_refusal(Park::Pause, Some(Target::Vagrant), Some("virtualbox")), None);
    assert!(parked_message(Park::Pause).starts_with("Lab paused."));
    assert!(parked_message(Park::Shutdown).starts_with("Lab shut down."));
}

#[tokio::test]
async fn a_pause_of_containers_on_docker_is_refused_before_anything_runs() {
    let t = TestApp::new();
    let spec =
        "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, os: debian@12, networks: { lab: 10 } }\n";
    t.lab("t", &[("isoloom.yml", spec)]);
    let tools = Tools::strict_rules(&[]);
    let err = park_lab(&t.dirs(), "t", Runtime::Docker, Park::Pause, |_| {}).await.unwrap_err();
    assert!(matches!(err, crate::error::Error::Refused(_)) && err.to_string().contains("shut it down instead"), "{err}");
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    assert_eq!(parked(&t.data().join("labs/t")), None, "not recorded as paused");
}

#[test]
fn a_cloud_lab_is_never_parked() {
    assert!(refuse_cloud_park(Runtime::Cloud).unwrap_err().to_string().starts_with("A cloud lab can't be paused"));
    assert!(refuse_cloud_park(Runtime::Docker).is_ok());
    assert!(refuse_cloud_park(Runtime::Vm).is_ok());
}

#[tokio::test]
async fn stopping_a_cloud_lab_that_runs_nowhere_clears_its_marks_without_running_anything() {
    // A cloud-services lab has nothing on this machine: a stop with no host marker (the
    // account was never recorded, or the marker was lost) only drops stale marks.
    let t = TestApp::new();
    let dir = t.lab("goat", &[]);
    mark_parked(&dir, Some(Park::Pause)).unwrap();
    let tools = Tools::strict_rules(&[]);
    stop_lab(&t.dirs(), "goat", Runtime::Cloud, std::future::ready(()), |_| {}).await.unwrap();
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    assert_eq!(parked(&dir), None);
    // Parking or resuming it is refused on the runtime alone.
    let err = park_lab(&t.dirs(), "goat", Runtime::Cloud, Park::Pause, |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
    let err = resume_lab(&t.dirs(), "goat", Runtime::Cloud, |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("A cloud lab can't be paused"), "{err}");
    assert_eq!(tools.calls(), Vec::<String>::new());
}

#[tokio::test]
async fn clearing_a_lab_that_is_not_parked_does_nothing() {
    let t = TestApp::new();
    let dir = t.lab("web-1", &[]);
    let tools = Tools::strict_rules(&[]);
    let mut lines = Vec::<String>::new();
    clear_parked(&t.dirs(), LabRef { dir: &dir, id: "web-1", runtime: Runtime::Vm }, &mut |l| lines.push(l)).await.unwrap();
    assert_eq!(lines, [] as [String; 0]);
    assert_eq!(tools.calls(), [] as [String; 0]);
}

/// What the page may do on lab `t`, declared `runtime`, where it is located now, on `provider`:
/// whether it pauses, shuts down, provisions and repairs.
fn controls_of(t: &TestApp, dir: &std::path::Path, runtime: Runtime, provider: Option<&str>) -> (bool, bool, bool, bool) {
    use crate::runtime::Control;
    let host = crate::runtime::host::locate(&t.dirs(), dir, "t", runtime).unwrap();
    let c = super::controls(host.as_ref(), provider);
    let has = |x| c.contains(&x);
    (has(Control::Pause), has(Control::Shutdown), has(Control::Provision), has(Control::Repair))
}

#[test]
fn the_page_is_offered_what_the_lifecycle_accepts_where_the_lab_runs() {
    use crate::runtime::paths::{mark_local_vm, mark_per_machine_vm};
    let t = TestApp::new();
    let _tools = Tools::strict_rules(&[]);
    let spec =
        "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, os: debian@12, networks: { lab: 10 } }\n";
    let dir = t.lab("t", &[("isoloom.yml", spec)]);
    // Containers here: shut down only (nothing to save), no provisioning; stopped containers
    // brought back as they are (regression, tests/resilience.md M4: a Docker lab with a
    // container stopped outside the app was offered a reset only).
    assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("docker")), (false, true, false, true));
    // A VM lab here: both, provisioning and the repair; QEMU can't suspend.
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("virtualbox")), (true, true, true, true));
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("qemu")), (false, true, true, true));
    // A container lab run as one VM per machine is a VM lab here, repaired as one too.
    mark_per_machine_vm(&dir, true).unwrap();
    assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("virtualbox")), (true, true, true, true));
    mark_per_machine_vm(&dir, false).unwrap();
    // Containers an earlier version runs inside a helper VM here: only stopped.
    mark_local_vm(&dir, Some(crate::runtime::providers::Provider::Virtualbox)).unwrap();
    assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("virtualbox")), (false, false, false, false));
    mark_local_vm(&dir, None).unwrap();
    // A cloud-services lab on no account: nothing runs.
    assert_eq!(controls_of(&t, &dir, Runtime::Cloud, None), (false, false, false, false));
    // In a cloud account, or on Proxmox: nothing parked or provisioned there.
    server::mark_lab(&dir, Some(&t.host("acct", "aws", serde_json::json!({ "useCliCreds": true })))).unwrap();
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("aws")), (false, false, false, false));
    crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
    for host in ["pve", "esx"] {
        crate::runtime::server::secrets::set_secret(host, "pw").unwrap();
    }
    server::mark_lab(&dir, Some(&t.host("pve", "proxmox", serde_json::json!({})))).unwrap();
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("proxmox")), (false, false, false, false));
    // On ESXi: provisioning a VM lab, through Vagrant there; never its containers' VM.
    server::mark_lab(&dir, Some(&t.host("esx", "vmware_esxi", serde_json::json!({})))).unwrap();
    assert_eq!(controls_of(&t, &dir, Runtime::Vm, Some("vmware_esxi")), (false, false, true, false));
    assert_eq!(controls_of(&t, &dir, Runtime::Docker, Some("vmware_esxi")), (false, false, false, false));
}
