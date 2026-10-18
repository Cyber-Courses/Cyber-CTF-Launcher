use super::*;
use crate::error::{Code, Wire};
use crate::testing::test_app::TestApp;

#[test]
fn a_recorded_reap_failure_is_read_back_for_the_lab_page() {
    // What a sweep couldn't destroy is kept per lab and shown on its page, so a lab billing past
    // its auto-stop never looks stopped.
    let t = TestApp::new();
    let dir = t.data().join(REAP_ERRORS);
    let wire = Wire { code: Code::Other, message: "the token has expired".into() };
    record_failure(&dir, "web", &wire, "Auto-stop couldn't destroy web: the token has expired").unwrap();
    assert_eq!(last_failure(t.handle(), "web"), Some(wire));
    assert_eq!(last_failure(t.handle(), "other"), None, "a lab that never failed has nothing");
}

#[test]
fn the_labs_that_failed_last_time_are_the_ones_with_a_record() {
    // The next sweep reads these before clearing them, so a lab that fails again isn't notified
    // twice and a lab destroyed since has its record go.
    let t = TestApp::new();
    let dir = t.data().join(REAP_ERRORS);
    let w = Wire { code: Code::Other, message: "x".into() };
    record_failure(&dir, "a", &w, "m").unwrap();
    record_failure(&dir, "b", &w, "m").unwrap();
    assert_eq!(failed_last_time(&dir), ["a".to_string(), "b".to_string()].into_iter().collect());
    assert!(failed_last_time(&t.data().join("never-swept")).is_empty());
}

#[test]
fn a_lab_id_must_be_a_plain_name_to_be_a_record_file() {
    assert!(valid_id("goad-light_2") && valid_id("web"));
    assert!(!valid_id("") && !valid_id("../x") && !valid_id(&"a".repeat(65)));
    let t = TestApp::new();
    assert_eq!(last_failure(t.handle(), "../secrets"), None, "a path is never read as a record");
}

#[tokio::test]
async fn a_sweep_of_a_fresh_app_destroys_nothing_and_records_nothing() {
    // No launcher deployment to hand over, no cloud run past its auto-stop: a clean no-op. The
    // runner-backed destroy of an expired cloud lab is covered by the fake-CLI and real-lab tests.
    let t = TestApp::new();
    reap_expired_labs(t.handle()).await;
    assert!(failed_last_time(&t.data().join(REAP_ERRORS)).is_empty());
}
