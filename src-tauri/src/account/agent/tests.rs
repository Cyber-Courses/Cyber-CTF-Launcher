use std::sync::Mutex;

use super::*;

/// The agent's dependencies against a test app: its data folder, the prompts it shows kept
/// in `prompts`, and a no-op notification (a system notification has nothing to show here).
pub(super) fn test_deps(t: &crate::testing::test_app::TestApp, prompts: Arc<Mutex<Vec<LaunchRequest>>>) -> Deps {
    let prompt = move |request: &LaunchRequest| prompts.lock().unwrap().push(request.clone());
    Deps::new(t.dirs(), Arc::new(prompt), Arc::new(|_: &str, _: &str| {}), Arc::new(|_: &str| Box::pin(async {}) as _))
}

#[test]
fn a_claim_is_read_whole_or_refused_saying_what_it_lacks() {
    // Regression: each field was read on its own with a default, so a claim without a lab id
    // or with a runtime this launcher doesn't know ran an empty id, or as containers.
    // The `claimLaunch` answer of lab "web" with `fields` set.
    let answer = |fields: Value| {
        let mut claim = claim_json("web");
        claim.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        json!({ "claimLaunch": claim })
    };
    let claim = parse_claim(&answer(json!({ "target": "aws1" }))).unwrap();
    let l = &claim.launch;
    assert_eq!((l.lab_id.as_str(), l.runtime, l.repository.as_str(), l.commit.as_str()), ("web", Runtime::Docker, "CyberCTF/web", COMMIT));
    assert_eq!(claim.target.as_deref(), Some("aws1"));
    let vm = parse_claim(&answer(json!({ "runtime": "VM", "target": null }))).unwrap();
    assert_eq!((vm.launch.runtime, vm.target), (Runtime::Vm, None));
    for (broken, missing) in [(json!({ "claimLaunch": { "runtime": "DOCKER" } }), "labId"), (answer(json!({ "runtime": "QUANTUM" })), "QUANTUM")] {
        let err = parse_claim(&broken).err().unwrap().to_string();
        assert!(err.starts_with("the website's launch request is incomplete: ") && err.contains(missing), "{err}");
    }
    assert!(parse_claim(&json!({ "claimLaunch": null })).is_err());
}

#[test]
fn a_launch_goes_to_the_target_picked_on_the_website_while_it_is_set_up() {
    let known = |t: &str| Ok((t == "esxi-1").then(|| "My ESXi".to_string()));
    assert_eq!(picked_target(Some("esxi-1"), known).unwrap().as_deref(), Some("esxi-1"));
    let gone = picked_target(Some("old"), known).unwrap_err();
    assert_eq!(gone.to_string(), "that host is no longer set up in the launcher");
    assert_eq!(picked_target(None, known).unwrap(), None);
    // Regression: saved hosts that can't be read said "no longer set up", as if it were gone.
    let unreadable = |_: &str| Err(Error::Invalid("the launcher's store: expected value".into()));
    let err = picked_target(Some("esxi-1"), unreadable).unwrap_err();
    assert_eq!(err.to_string(), "the launcher's store: expected value");
}

#[tokio::test]
async fn a_launch_runs_on_its_picked_host_else_here() {
    let t = crate::testing::test_app::TestApp::new();
    let az = t.host("az1", "azure", json!({}));
    let on_az = Claim { target: Some(az), ..claim_of("web") };
    assert_eq!(launch_place(&t.dirs(), &on_az).await.unwrap().host.as_deref(), Some("az1"));
    assert_eq!(launch_place(&t.dirs(), &claim_of("web")).await.unwrap(), StartPlace::default(), "here");
}

#[tokio::test]
async fn a_lab_that_was_never_installed_has_nothing_to_tear_down() {
    // A lab whose download failed: the failure isn't said to have left anything up.
    let t = crate::testing::test_app::TestApp::new();
    let deps = test_deps(&t, Arc::default());
    let tools = crate::platform::exec::fake::Tools::strict_rules(&[]);
    tear_down(&deps, "gone", Runtime::Docker).await.unwrap();
    tear_down(&deps, "gone", Runtime::Vm).await.unwrap();
    let err = undo_start(&deps, "gone", Runtime::Docker, false, Error::Invalid("download failed: HTTP 404".into())).await;
    assert_eq!(err.to_string(), "download failed: HTTP 404");
    assert_eq!(tools.calls(), [] as [&str; 0]);
}

pub(super) const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// A `claimLaunch` answer of lab `id` (Docker) at another commit than the one installed.
fn claim_json(id: &str) -> Value {
    json!({ "labId": id, "runtime": "DOCKER", "repository": "CyberCTF/web", "commit": COMMIT, "env": [] })
}

/// A website claim of lab `id` (Docker) at another commit than the one installed, no target.
pub(super) fn claim_of(id: &str) -> Claim {
    parse_claim(&json!({ "claimLaunch": claim_json(id) })).unwrap()
}

#[tokio::test]
async fn a_refused_website_launch_leaves_the_lab_running_on_the_players_account() {
    // Regression: the agent asked only whether the lab ran *here*. Up on the player's AWS
    // account and launched again from the website after a new version was published, the
    // refusal ("still running on <host>. Stop it, then start it again") was followed by a
    // teardown that ran `terraform destroy` on their lab.
    let t = crate::testing::test_app::TestApp::new();
    let dirs = t.dirs();
    let id = "agent-refused";
    t.lab(id, &[(crate::runtime::lab::COMMIT_MARKER, "ffffffffffffffffffffffffffffffffffffffff"), (".isoloom/cloud-docker/aws/main.tf", "")]);
    let aws = t.host("aws1", "aws", json!({ "useCliCreds": true }));
    crate::runtime::test_support::record_run(t.data(), id, Some(&aws), "cloud-docker", "ready");
    let tools = crate::platform::exec::fake::Tools::strict_rules(&[("docker network ls", crate::platform::exec::fake::ok(""))]);
    assert!(crate::runtime::lab_present(&dirs, id));
    let err = run_claimed(&test_deps(&t, Arc::default()), &claim_of(id).launch, &StartPlace::default(), false).await.unwrap_err();
    assert!(err.is_refusal() && err.to_string().contains("still running on Host aws1"), "{err}");
    assert!(!tools.called("terraform"), "nothing torn down: {:?}", tools.calls());
    assert_eq!(crate::runtime::recorded::Recorded::of(&dirs, id).unwrap().host(), Some("aws1"));
}

#[tokio::test]
async fn a_failed_start_of_a_lab_that_was_already_there_is_not_torn_down() {
    // A start that fails on a lab something of which was there before (here, a lab up on a
    // Proxmox host whose start this time failed for another reason) stops nothing.
    let t = crate::testing::test_app::TestApp::new();
    let id = "agent-present";
    t.lab(id, &[]);
    crate::runtime::test_support::record_run(t.data(), id, Some("pve1"), "proxmox", "ready");
    let tools = crate::platform::exec::fake::Tools::strict_rules(&[]);
    let failure = Error::Invalid("the lab host didn't answer".into());
    let err = undo_start(&test_deps(&t, Arc::default()), id, Runtime::Docker, crate::runtime::lab_present(&t.dirs(), id), failure).await;
    assert_eq!(err.to_string(), "the lab host didn't answer");
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    assert_eq!(crate::runtime::recorded::Recorded::of(&t.dirs(), id).unwrap().host(), Some("pve1"));
}

#[test]
fn only_a_start_that_brought_something_up_is_undone() {
    assert!(undoes(&Error::Invalid("unhealthy".into()), false));
    assert!(!undoes(&Error::Invalid("unhealthy".into()), true), "the lab was there before");
    assert!(!undoes(&Error::Refused("still running on AWS".into()), false), "refused before anything changed");
    assert!(!undoes(&Error::coded(crate::error::Code::NoHypervisor), false), "nowhere to run: nothing started");
    assert!(!undoes(&Error::coded(crate::error::Code::PickCloudAccount), false), "nowhere to run: nothing started");
}

#[test]
fn a_session_update_carries_its_state_and_progress() {
    let p = Progress { local_url: Some("http://127.0.0.1:8080"), message: Some("Running on your machine"), ..Default::default() };
    let vars = update_vars("s-1", "RUNNING", &p);
    assert_eq!(vars["id"], "s-1");
    assert_eq!(vars["s"], "RUNNING");
    assert_eq!(vars["u"], "http://127.0.0.1:8080");
    assert_eq!(vars["m"], "Running on your machine");
    assert!(vars["c"].is_null() && vars["t"].is_null() && vars["n"].is_null());
    let p = Progress::default();
    assert!(p.local_url.is_none() && p.control_url.is_none() && p.token.is_none() && p.nonce.is_none() && p.message.is_none());
}

#[test]
fn the_running_message_names_the_host_or_this_machine() {
    assert_eq!(running_message(Some("My Proxmox".into())), "Running on My Proxmox");
    assert_eq!(running_message(None), "Running on your machine");
}

#[tokio::test]
async fn the_agents_failures_with_the_website_are_logged_not_dropped() {
    // Regression: a failed poll, a session that couldn't be marked failed and a co-location
    // check that couldn't start were all dropped without a word in the log.
    let t = crate::testing::test_app::TestApp::new();
    crate::testing::logs::take_warnings();
    Box::pin(poll_once(&test_deps(&t, Arc::default()), "agent-1")).await;
    report_failed("s-1".into(), "it broke".into()).await;
    assert_eq!(control_url("web", Err(Error::Invalid("no free port".into()))), None);
    assert_eq!(control_url("web", Ok(47100)).as_deref(), Some("http://127.0.0.1:47100"));
    let logged = crate::testing::logs::take_warnings();
    assert!(logged[0].starts_with("myPendingLaunches failed: "), "{logged:?}");
    assert!(logged[1].starts_with("lab session s-1 couldn't be marked failed on the website: "), "{logged:?}");
    assert_eq!(logged[2], "the co-location check of web couldn't start: no free port");
    assert_eq!(logged.len(), 3, "{logged:?}");
}

#[test]
fn pending_launches_are_read_by_id() {
    let data = json!({ "myPendingLaunches": [{ "id": "s-1" }, { "other": 1 }, { "id": 7 }, { "id": "s-2" }] });
    assert_eq!(pending_ids(&data), ["s-1", "s-2"]);
    assert_eq!(pending_ids(&json!({ "myPendingLaunches": null })), [] as [&str; 0]);
    assert_eq!(pending_ids(&serde_json::Value::Null), [] as [&str; 0]);
}

#[tokio::test]
async fn a_failed_claim_is_reported_and_the_next_one_still_runs() {
    let claimed = Mutex::new(Vec::new());
    let failed = Mutex::new(Vec::new());
    let ids = vec!["s-1".to_string(), "s-2".to_string(), "s-3".to_string()];
    let claim = |sid: String| {
        claimed.lock().unwrap().push(sid.clone());
        async move { if sid == "s-2" { Err(Error::Refused("Web wasn't approved on this machine, so it didn't start on AWS.".into())) } else { Ok(()) } }
    };
    let fail = |sid: String, why: String| {
        failed.lock().unwrap().push((sid, why));
        async {}
    };
    claim_each(ids, claim, fail).await;
    assert_eq!(*claimed.lock().unwrap(), ["s-1", "s-2", "s-3"]);
    assert_eq!(*failed.lock().unwrap(), [("s-2".to_string(), "Web wasn't approved on this machine, so it didn't start on AWS.".to_string())]);
}
