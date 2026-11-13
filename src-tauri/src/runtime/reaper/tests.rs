use super::*;
use crate::error::{Code, Wire};
use crate::testing::test_app::TestApp;

/// The labs the store keeps a failure for.
fn failed(t: &TestApp) -> Vec<String> {
    crate::store::open(&t.dirs()).unwrap().read(crate::store::autostop::failed).unwrap()
}

/// Keeps `failures` (lab, error) as a sweep would.
fn record(t: &TestApp, failures: &[(&str, &Wire)]) {
    let rows: Vec<(String, String, Wire)> =
        failures.iter().map(|(id, w)| ((*id).to_owned(), format!("Auto-stop couldn't destroy {id}"), (*w).clone())).collect();
    crate::store::open(&t.dirs()).unwrap().write(|tx| crate::store::autostop::replace(tx, &rows)).unwrap();
}

#[test]
fn a_recorded_reap_failure_is_read_back_for_the_lab_page() {
    // What a sweep couldn't destroy is kept per lab and shown on its page, so a lab billing past
    // its auto-stop never looks stopped.
    let t = TestApp::new();
    let wire = Wire { code: Code::Other, message: "the token has expired".into() };
    record(&t, &[("web", &wire)]);
    assert_eq!(last_failure(&t.dirs(), "web"), Some(wire));
    assert_eq!(last_failure(&t.dirs(), "other"), None, "a lab that never failed has nothing");
}

#[test]
fn a_sweeps_failures_replace_the_last_ones() {
    // The next sweep reads these before replacing them, so a lab that fails again isn't notified
    // twice and a lab destroyed since has its record go.
    let t = TestApp::new();
    let w = Wire { code: Code::Other, message: "x".into() };
    record(&t, &[("a", &w), ("b", &w)]);
    assert_eq!(failed(&t), ["a", "b"]);
    record(&t, &[("b", &w)]);
    assert_eq!(failed(&t), ["b"]);
}

#[test]
fn a_lab_id_must_be_a_plain_name_to_be_a_record_file() {
    let t = TestApp::new();
    assert_eq!(last_failure(&t.dirs(), "../secrets"), None, "a path is never read as a record");
}

#[tokio::test]
async fn a_sweep_of_a_fresh_app_destroys_nothing_and_records_nothing() {
    // No launcher deployment to hand over, no cloud run past its auto-stop: a clean no-op. The
    // runner-backed destroy of an expired cloud lab is covered by the fake-CLI and integration tests.
    let t = TestApp::new();
    reap_expired_labs(&t.dirs(), &|_: &str, _: &str| {}).await;
    assert_eq!(failed(&t), [] as [String; 0]);
}

#[test]
fn a_deployment_the_runner_cant_take_is_said_to_need_removing_by_hand() {
    // Regression: the player was told Stop and the auto-stop would still remove it, though the
    // launcher no longer runs Terraform itself: it kept billing.
    let m = handover_message("web", Provider::Aws, &"its lab isn't installed any more");
    assert!(m.contains("the launcher can't remove it") && m.contains("delete its resources in your AWS console"), "{m}");
    assert!(!m.contains("still remove it"), "{m}");
    let pve = handover_message("web", Provider::Proxmox, &"the runner refused it");
    assert!(pve.contains("delete its VMs on the Proxmox server") && !pve.contains("costing"), "{pve}");
}
