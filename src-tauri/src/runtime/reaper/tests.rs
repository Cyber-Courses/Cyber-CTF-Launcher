use super::*;
use crate::exec::fake::{Tools, fail};
use crate::test_app::TestApp;

#[test]
fn the_reaper_finds_expired_states_left_on_another_host() {
    // Regression: the reaper only looked at the state of the host a lab's marker named, so a
    // cloud deployment left behind when the lab was started elsewhere was never destroyed.
    let base = std::env::temp_dir().join(format!("cyberctf-reap-{}", rand::random::<u32>()));
    let state = |id: &str, target: &str, expires_at: u64| {
        let s = base.join("deployments").join(id).join(target);
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}},"resources":[{}]}"#).unwrap();
        std::fs::write(s.join("run.json"), format!(r#"{{"expires_at":{expires_at}}}"#)).unwrap();
        s
    };
    let left = state("web-1", "aws", 1);
    state("web-1", "proxmox", u64::MAX);
    let other = state("db-1", "azure", 1);
    let mut found = expired_states(&base);
    found.sort();
    assert_eq!(found, [("db-1".to_string(), "azure".to_string(), other), ("web-1".to_string(), "aws".to_string(), left)]);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn an_expired_state_is_destroyed_with_the_host_it_was_applied_on() {
    // The lab still runs where its marker says: the usual stop.
    assert_eq!(reap_owner(Some(("aws", "aws1")), Some("aws1"), "aws"), Some(Owner::Current));
    assert_eq!(reap_owner(Some(("aws", "aws1")), None, "aws"), Some(Owner::Current), "a state from before hosts were recorded");
    // The lab moved to Proxmox (or this machine): the AWS state is a leftover of aws1.
    assert_eq!(reap_owner(Some(("proxmox", "pve1")), Some("aws1"), "aws"), Some(Owner::Left("aws1".into())));
    assert_eq!(reap_owner(None, Some("aws1"), "aws"), Some(Owner::Left("aws1".into())));
    assert_eq!(reap_owner(Some(("aws", "aws2")), Some("aws1"), "aws"), Some(Owner::Left("aws1".into())));
    // Nothing says which credentials can destroy it.
    assert_eq!(reap_owner(None, None, "aws"), None);
    assert_eq!(reap_owner(Some(("proxmox", "pve1")), None, "aws"), None);
}

#[tokio::test]
async fn the_reaper_only_looks_at_expired_cloud_labs() {
    let t = TestApp::new();
    let tools = Tools::rules(&[]);
    reap_expired_labs(t.handle()).await;
    // A deployment whose lab isn't installed, one on no host, one not expired yet.
    for id in ["reap-none-gone", "reap-none-here", "reap-none-fresh"] {
        std::fs::create_dir_all(t.data().join("deployments").join(id).join("azure")).unwrap();
    }
    t.lab("reap-none-here", &[]);
    let fresh = t.lab("reap-none-fresh", &[]);
    server::mark_lab(&fresh, Some(&t.host("az1", "azure", serde_json::json!({})))).unwrap();
    reap_expired_labs(t.handle()).await;
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    assert_eq!(last_failure(t.handle(), "reap-none-fresh"), None);
}

/// A lab `id` installed with an Azure Isoloom module, running on account `az1`, and its
/// deployment there past its stop time (with a billable resource). Returns the state folder.
fn expired_on_azure(t: &TestApp, id: &str) -> PathBuf {
    let dir = t.lab(id, &[(".isoloom/cloud-docker/azure/main.tf", "")]);
    server::mark_lab(&dir, Some(&t.host("az1", "azure", serde_json::json!({})))).unwrap();
    let state = t.data().join("deployments").join(id).join("azure");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("run.json"), r#"{"expires_at": 1}"#).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"resources": [{"type": "azurerm_linux_virtual_machine"}]}"#).unwrap();
    std::fs::write(state.join("host"), "az1").unwrap();
    state
}

#[tokio::test]
async fn a_destroy_the_reaper_cant_do_is_kept_and_shown_until_it_succeeds() {
    // Regression: a failed auto-stop destroy was dropped (`let _ =`). Expired Azure credentials
    // made every sweep fail the same way, unnoticed, while the VM kept billing. (Lab ids are
    // unique per test: the lab lock is the process's.)
    let t = TestApp::new();
    let state = expired_on_azure(&t, "reap-fails");
    let failing = Tools::rules(&[("terraform destroy", fail("AADSTS700082: The refresh token has expired due to inactivity."))]);
    reap_expired_labs(t.handle()).await;
    assert!(failing.called("terraform destroy"), "{:?}", failing.calls());
    let shown = last_failure(t.handle(), "reap-fails").expect("the failure is kept for the lab's page");
    assert!(matches!(&shown.code, Code::ReapDestroy { account, .. } if account == "Host az1"), "{shown:?}");
    let shown = shown.message;
    assert!(shown.starts_with("Auto-stop couldn't destroy reap-fails: destroying it on Host az1 failed:") && shown.contains("AADSTS700082"), "{shown}");
    assert!(state.join("terraform.tfstate").is_file(), "kept for the next sweep");
    drop(failing);
    // The credentials renewed: the next sweep destroys it, and the failure is gone with it.
    let tools = Tools::rules(&[]);
    reap_expired_labs(t.handle()).await;
    assert!(tools.called("terraform destroy"));
    assert!(!state.join("terraform.tfstate").exists() && !state.join(REAP_ERROR).exists());
    assert_eq!(last_failure(t.handle(), "reap-fails"), None);
}

#[tokio::test]
async fn what_used_to_be_skipped_silently_is_recorded() {
    // Regression: a lab no longer installed, an account removed, or a state naming no account
    // was a silent `continue`: the deployment kept billing and nothing said so.
    let t = TestApp::new();
    let _tools = Tools::rules(&[]);
    let expired = |id: &str, host: Option<&str>| {
        let state = t.data().join("deployments").join(id).join("aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("run.json"), r#"{"expires_at": 1}"#).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"resources": [{"type": "aws_instance"}]}"#).unwrap();
        if let Some(h) = host {
            std::fs::write(state.join("host"), h).unwrap();
        }
        state
    };
    let uninstalled = expired("reap-gone-lab", Some("aws1"));
    t.lab("reap-orphan", &[]);
    expired("reap-orphan", None);
    t.lab("reap-moved", &[]);
    expired("reap-moved", Some("removed-account"));
    reap_expired_labs(t.handle()).await;
    let recorded = |state: &Path| last_failure_at(state, Some(1)).map(|w| w.message).unwrap_or_default();
    assert!(recorded(&uninstalled).contains("its lab files are gone"), "{}", recorded(&uninstalled));
    assert!(last_failure(t.handle(), "reap-orphan").unwrap().message.contains("no account is recorded for it"));
    assert!(last_failure(t.handle(), "reap-moved").unwrap().message.contains("the account removed-account can't be used"));
}

#[test]
fn a_failure_is_notified_once_and_only_shown_for_the_stop_time_it_was_about() {
    let state = std::env::temp_dir().join(format!("cyberctf-reap-err-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("run.json"), r#"{"expires_at": 7}"#).unwrap();
    let expired_creds = ReapFailure::Destroy("My Azure".into(), Error::Invalid("token expired".into()));
    assert!(record_failure("web", &state, &expired_creds), "new: notified");
    assert!(!record_failure("web", &state, &expired_creds), "the same again: not notified again");
    // The same failure in other words (the tool's request id changes on every try): not new.
    let again = ReapFailure::Destroy("My Azure".into(), Error::Invalid("token expired (RequestId: 42)".into()));
    assert!(!record_failure("web", &state, &again));
    assert!(last_failure_at(&state, Some(7)).unwrap().message.contains("RequestId: 42"), "the latest words are shown");
    assert!(record_failure("web", &state, &ReapFailure::TimedOut), "another failure is new");
    let timed_out = last_failure_at(&state, Some(7)).unwrap();
    assert_eq!(timed_out.message, failure_message("web", &ReapFailure::TimedOut));
    assert_eq!(timed_out.code, Code::ReapTimedOut { minutes: 35 });
    // Launched again since (a new stop time): the old failure isn't this lab's any more.
    assert_eq!(last_failure_at(&state, Some(8)), None);
    assert_eq!(last_failure_at(&state, None), None);
    // A record from before failures had codes is said in its own words.
    std::fs::write(state.join(REAP_ERROR), r#"{"expires_at": 7, "kind": "timed-out", "message": "Auto-stop couldn't destroy web"}"#).unwrap();
    assert_eq!(last_failure_at(&state, Some(7)), Some(Wire { code: Code::Other, message: "Auto-stop couldn't destroy web".into() }));
    forget_failure(&state);
    assert_eq!(last_failure_at(&state, Some(7)), None);
    std::fs::remove_dir_all(state).unwrap();
}

#[tokio::test]
async fn a_hung_reap_is_given_up_so_the_sweep_goes_on() {
    let r = within(Duration::from_millis(30), std::future::pending()).await;
    assert!(matches!(r, Err(ReapFailure::TimedOut)), "{r:?}");
    assert_eq!(within(Duration::from_secs(5), async { Ok(Reaped::Skipped) }).await.unwrap(), Reaped::Skipped);
    assert!(ReapFailure::TimedOut.to_string().contains("35 minutes"));
    assert!(REAP_TIMEOUT > terraform::DESTROY_TIMEOUT, "Terraform's own limit says more, so it runs out first");
}

/// A cloud-services lab `id` running on the AWS account `aws1`, its auto-stop at `expires_at`:
/// the lab folder and its Terraform state folder. Each test has a lab id of its own: the lab lock
/// is process-wide, and one test holds it on purpose.
const CLOUD_SPEC: &str =
    "version: 1\nname: goat\ncloud:\n  provider: aws\n  terraform: terraform\n  vars: { region: us-east-1 }\n  outputs: { app: app_url }\n";

fn cloud_lab(t: &TestApp, id: &str, expires_at: u64) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = t.lab(id, &[("isoloom.yml", CLOUD_SPEC), ("terraform/main.tf", "variable \"region\" {}\n")]);
    let host = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    server::mark_lab(&dir, Some(&host)).unwrap();
    let state = t.data().join("deployments").join(id).join("aws");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[{"instances":[{}]}],"outputs":{}}"#).unwrap();
    std::fs::write(state.join("run.json"), format!(r#"{{"expires_at":{expires_at}}}"#)).unwrap();
    std::fs::write(state.join("runtime"), "CLOUD").unwrap();
    std::fs::write(state.join("host"), "aws1").unwrap();
    (dir, state)
}

#[tokio::test]
async fn the_reaper_destroys_a_cloud_lab_that_expires_this_very_second() {
    // Expiry is inclusive: at `expires_at` itself the lab is destroyed, not left for a tick.
    let t = TestApp::new();
    let (dir, state) = cloud_lab(&t, "reap-now", crate::runtime::terraform::now());
    let tools = Tools::rules(&[]);
    reap_expired_labs(t.handle()).await;
    assert!(tools.called("terraform destroy"), "{:?}", tools.calls());
    assert!(!state.join("terraform.tfstate").exists(), "destroyed with the account it ran on");
    assert_eq!(server::marked_host(&dir), None, "no longer running there");
    // Nothing left to reap: a second sweep runs nothing.
    drop(tools);
    let tools = Tools::rules(&[]);
    reap_expired_labs(t.handle()).await;
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

#[tokio::test]
async fn the_reaper_leaves_a_lab_another_operation_holds_for_the_next_sweep() {
    let t = TestApp::new();
    let (dir, state) = cloud_lab(&t, "reap-held", 1);
    let tools = Tools::rules(&[]);
    // A stop (or a deploy worker) holds the lab: destroying under it could corrupt the state.
    let held = crate::runtime::inflight::lock_lab(t.handle(), "reap-held").await.unwrap();
    reap_expired_labs(t.handle()).await;
    assert!(!tools.called("terraform"), "{:?}", tools.calls());
    assert!(state.join("terraform.tfstate").is_file());
    assert_eq!(server::marked_host(&dir).as_deref(), Some("aws1"));
    drop(held);
    reap_expired_labs(t.handle()).await;
    assert!(tools.called("terraform destroy"), "{:?}", tools.calls());
    assert!(!state.join("terraform.tfstate").exists());
}

#[tokio::test]
async fn the_reaper_skips_an_expired_leftover_whose_host_was_removed() {
    // The state names a host that is no longer saved: nothing can destroy it from here, and
    // the sweep must neither stop nor touch the state (the host may be added back).
    let t = TestApp::new();
    let (dir, state) = cloud_lab(&t, "reap-gone", 1);
    server::mark_lab(&dir, None).unwrap();
    std::fs::write(state.join("host"), "removed-host").unwrap();
    let tools = Tools::rules(&[]);
    reap_expired_labs(t.handle()).await;
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    assert!(state.join("terraform.tfstate").is_file());
    // Not silently, though: the lab's page says why it still bills.
    assert!(last_failure(t.handle(), "reap-gone").unwrap().message.contains("the account removed-host can't be used"));
    // Same when the lab itself is uninstalled: its state alone can't say how to destroy it.
    std::fs::remove_dir_all(&dir).unwrap();
    reap_expired_labs(t.handle()).await;
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    assert!(state.join("terraform.tfstate").is_file());
}
