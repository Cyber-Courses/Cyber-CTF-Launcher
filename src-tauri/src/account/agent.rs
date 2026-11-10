//! Launcher agent: "bring your own compute". Once the player is logged in, this machine
//! registers itself with CyberBackend (keyed by a stable per-install id so several
//! machines are distinct), heartbeats so it shows as online, and polls for the player's
//! launch requests. When one arrives it claims it, asks the player on this machine (the lab, where
//! it would run, whether it has privileged machines), runs it once they approve, and reports back.
//! A launch nobody approves here never runs. The website checks that a running lab's local
//! address is this machine through the co-location probe (`colocation`).
//!
//! An administrator's policy (`DisableWebsiteAgent`) turns the agent off: the machine then
//! never registers, shows online, nor takes launches.
//!
//! Here: a claimed launch, from the claim to the report back. This machine's identity is
//! `identity`'s; its registration, the heartbeat and the agent loop are `registration`'s; the
//! player's approval is `confirm`'s.

mod confirm;
mod identity;
mod registration;

use std::sync::Arc;

use crate::platform::AppDirs;
use serde_json::{Value, json};

pub use confirm::{LaunchRequest, PromptLaunch, confirm_launch};
use confirm::{confirm_here, launch_request};
use identity::random_hex;
pub use registration::{AgentInfo, agent_info, run};

use crate::account::api;
use crate::account::colocation;
use crate::error::{Error, Result};
use crate::labs;
use crate::runtime::start_place::{self, StartPlace};
use crate::runtime::{self, Runtime, server};

/// A system notification for the player (title, body), shown when a website launch starts here.
pub type Notify = dyn Fn(&str, &str) + Send + Sync;

/// Ends the deploy still running in lab `id`'s worker, if any, before the agent stops the lab
/// (see `labs::stop_lab`).
pub type EndWorkers = dyn Fn(&str) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync;

/// The app the background agent runs inside: its data folder, the two things it reaches the
/// player through — a prompt to confirm a website launch and a system notification when one
/// starts here —, and the ending of a lab's deploy workers. The app shell builds it from the `AppHandle` (see `crate::app`); unit tests build
/// one with effects of their own. Keeping the effects behind closures is what lets this module
/// stay domain code, free of `tauri`.
#[derive(Clone)]
pub struct Deps {
    dirs: AppDirs,
    prompt_launch: Arc<PromptLaunch>,
    notify: Arc<Notify>,
    end_workers: Arc<EndWorkers>,
}

impl Deps {
    /// The agent's dependencies: the data folder, the launch prompt, the notification, and the
    /// ending of a lab's workers.
    pub fn new(dirs: AppDirs, prompt_launch: Arc<PromptLaunch>, notify: Arc<Notify>, end_workers: Arc<EndWorkers>) -> Deps {
        Deps { dirs, prompt_launch, notify, end_workers }
    }

    fn dirs(&self) -> &AppDirs {
        &self.dirs
    }
}

#[derive(Default)]
struct Progress<'a> {
    local_url: Option<&'a str>,
    control_url: Option<&'a str>,
    token: Option<&'a str>,
    nonce: Option<&'a str>,
    message: Option<&'a str>,
}

async fn update_state(session_id: &str, state: &str, p: Progress<'_>) -> Result<()> {
    api::graphql(
        "mutation ($id: ID!, $s: LabSessionState!, $u: String, $c: String, $t: String, $n: String, $m: String) { updateLabSession(sessionId: $id, state: $s, localUrl: $u, controlUrl: $c, localToken: $t, coLocationNonce: $n, message: $m) { id } }",
        update_vars(session_id, state, &p),
        true,
    )
    .await
    .map(|_| ())
}

/// The `updateLabSession` variables for a session moving to `state`.
fn update_vars(session_id: &str, state: &str, p: &Progress<'_>) -> Value {
    json!({ "id": session_id, "s": state, "u": p.local_url, "c": p.control_url, "t": p.token, "n": p.nonce, "m": p.message })
}

/// A launch the website sent this machine, as `claimLaunch` answers it: the lab to download and
/// run, and the host or cloud account the player picked for it, if any.
#[derive(Clone, serde::Deserialize)]
pub(super) struct Claim {
    #[serde(flatten)]
    launch: labs::Launch,
    target: Option<String>,
}

/// The claim in a `claimLaunch` answer; one missing what a launch needs is an error saying so,
/// never a launch of an empty lab id or of the wrong runtime.
fn parse_claim(data: &Value) -> Result<Claim> {
    serde_json::from_value(data["claimLaunch"].clone()).map_err(|e| Error::Invalid(format!("the website's launch request is incomplete: {e}")))
}

/// Where a claimed launch runs: the target picked on the website (one of this launcher's hosts
/// or cloud accounts); else where the app's Start with no place chosen runs it
/// ([`start_place::default_place`]: where it last ran, else a ready hypervisor here for a VM lab,
/// else the default server). A cloud lab never runs on an account it wasn't sent to (it costs
/// money): no target, no launch (`runtime::start` says so).
async fn launch_place(dirs: &AppDirs, claim: &Claim) -> Result<StartPlace> {
    if let Some(target) = picked_target(claim.target.as_deref(), |target| server::host_name(dirs, target))? {
        return Ok(StartPlace { host: Some(target), ..StartPlace::default() });
    }
    let l = &claim.launch;
    // The player's preferred hypervisor is the webview's (Settings): a website launch takes the
    // first ready one.
    start_place::default_place(dirs, &l.lab_id, l.runtime, labs::source(&l.repository, &l.commit), None).await
}

/// The target picked on the website, given how to name a host (`None` when it isn't set up):
/// none when it picked none, an error when the host is gone.
fn picked_target(target: Option<&str>, host_name: impl Fn(&str) -> Result<Option<String>>) -> Result<Option<String>> {
    let Some(target) = target else { return Ok(None) };
    host_name(target)?.map(|_| Some(target.to_string())).ok_or_else(|| Error::Invalid("that host is no longer set up in the launcher".into()))
}

/// Stops a claimed lab that must not keep running under a failed session. A lab that never got
/// installed (its download failed) has nothing to stop.
async fn tear_down(deps: &Deps, lab_id: &str, runtime: Runtime) -> Result<()> {
    let dirs = deps.dirs();
    if crate::runtime::lab_installed(dirs, lab_id) {
        labs::stop_lab(dirs, lab_id, runtime, (deps.end_workers)(lab_id), |_line: String| {}).await
    } else {
        Ok(())
    }
}

/// `failure`, after tearing down what the failed start brought up, when it must be (see
/// [`undoes`]). A teardown that fails too is said with it: the lab may still be running (and,
/// on a cloud account, billing), and the website and the log must say so.
async fn undo_start(deps: &Deps, lab_id: &str, runtime: Runtime, present_before: bool, failure: Error) -> Error {
    if !undoes(&failure, present_before) {
        return failure;
    }
    match tear_down(deps, lab_id, runtime).await {
        Ok(()) => failure,
        Err(e) => {
            log::warn!("stopping what a failed launch of {lab_id} started: {e}");
            Error::Invalid(format!("{failure} Stopping what it had started failed too ({e}): stop the lab from its page."))
        }
    }
}

/// Whether a failed start is undone: only what it brought up is. A start refused before it
/// changed anything (still running elsewhere, a new version of a running lab, already up) left
/// the lab as it was, and so did any start of a lab something of which was already there
/// (`present_before`: up on a server host or cloud account, parked, its VMs): tearing it down
/// would destroy the player's running lab, the opposite of what the refusal tells them.
fn undoes(failure: &Error, present_before: bool) -> bool {
    !present_before && !failure.is_refusal()
}

/// Starts a claimed lab at `place` (no host: this machine). A failure tears down what this
/// start brought up, and only that (see [`undoes`]; `present_before`: whether something of it
/// was there before, see `runtime::lab_present`). Where the lab is reachable here, once running.
async fn run_claimed(deps: &Deps, launch: &labs::Launch, place: &StartPlace, present_before: bool) -> Result<Option<String>> {
    let to =
        runtime::Destination { host: place.host.as_deref(), provider: place.provider, per_machine_vm: place.per_machine_vm, ..runtime::Destination::default() };
    match labs::run(deps.dirs(), launch.clone(), to, |_line: String| {}).await {
        Ok(url) => Ok(url),
        Err(e) => Err(undo_start(deps, &launch.lab_id, launch.runtime, present_before, e).await),
    }
}

/// Tells the backend the lab runs: where its target is reachable, plus a loopback control
/// endpoint + one-time token/nonce so the website can verify co-location before trusting the
/// 127.0.0.1 URL (the endpoint ends once it answered, when the lab stops, or after a while).
async fn report_running(dirs: &AppDirs, session_id: &str, lab_id: &str, host: Option<&str>, url: Option<&str>) -> Result<()> {
    // On a host: its name, else (no longer saved, or the saved hosts unreadable) its id, never
    // "this machine".
    let running_on = running_message(host.map(|h| server::host_name(dirs, h).ok().flatten().unwrap_or_else(|| h.to_string())));
    let token = random_hex();
    let nonce = random_hex();
    let control_url = control_url(lab_id, colocation::serve(lab_id, token.clone(), nonce.clone()).await);
    let progress = Progress { local_url: url, control_url: control_url.as_deref(), token: Some(&token), nonce: Some(&nonce), message: Some(&running_on) };
    update_state(session_id, "RUNNING", progress).await
}

/// The co-location server's address, from the port it was `served` on. Without one the website
/// can't check the lab runs here, but the lab still runs: logged, not fatal.
fn control_url(lab_id: &str, served: Result<u16>) -> Option<String> {
    served.map(|port| format!("http://127.0.0.1:{port}")).inspect_err(|e| log::warn!("the co-location check of {lab_id} couldn't start: {e}")).ok()
}

/// Where the website says the lab runs: the host's name, or this machine.
fn running_message(host_name: Option<String>) -> String {
    host_name.map_or_else(|| "Running on your machine".to_string(), |n| format!("Running on {n}"))
}

/// Claims one pending session and runs its lab on this machine.
async fn claim_and_run(deps: &Deps, session_id: &str) -> Result<()> {
    let data = api::graphql(
        "mutation ($id: ID!) { claimLaunch(sessionId: $id) { labId runtime repository commit target env { name value } } }",
        json!({ "id": session_id }),
        true,
    )
    .await?;
    let claim = parse_claim(&data)?;
    let place = launch_place(deps.dirs(), &claim).await?;
    let host = place.host.clone();
    let request = launch_request(deps.dirs(), session_id, &claim.launch, host.as_deref()).await?;
    update_state(session_id, "PULLING", Progress { message: Some("Waiting for approval on your machine"), ..Default::default() }).await?;
    confirm_here(deps, &request).await?;
    let launch = &claim.launch;
    // Anything this start brought up before failing (one unhealthy service, say) is torn down, or
    // it would keep running under a FAILED session and every later launch of it would be refused.
    let present = crate::runtime::lab_present(deps.dirs(), &launch.lab_id);
    let url = run_claimed(deps, launch, &place, present).await?;
    // The lab is actually running now. If reporting back to the backend fails, tear down what
    // this start brought up before returning the error, so we don't leave infra running under a
    // session the poller will mark FAILED.
    if let Err(e) = report_running(deps.dirs(), session_id, &launch.lab_id, host.as_deref(), url.as_deref()).await {
        return Err(undo_start(deps, &launch.lab_id, launch.runtime, present, e).await);
    }
    // A lab launched from the website just started: let the player know on this machine.
    (deps.notify)("Lab running", &running_notice(&request));
    Ok(())
}

/// The notification's text once the lab of `request` runs, naming where it runs.
fn running_notice(request: &LaunchRequest) -> String {
    format!("A lab launched from the website is now running on {}.", request.place_name())
}

/// Picks up and runs every pending launch aimed at this agent. A failed read of them is logged;
/// the next poll asks again.
pub(super) async fn poll_once(deps: &Deps, agent_id: &str) {
    match api::graphql("query ($a: ID) { myPendingLaunches(agentId: $a) { id } }", json!({ "a": agent_id }), true).await {
        Ok(data) => claim_each(pending_ids(&data), |sid| async move { Box::pin(claim_and_run(deps, &sid)).await }, report_failed).await,
        Err(e) => log::warn!("myPendingLaunches failed: {e}"),
    }
}

/// The session ids in a `myPendingLaunches` answer (entries without one are skipped).
fn pending_ids(data: &Value) -> Vec<String> {
    let sessions = data["myPendingLaunches"].as_array().map(Vec::as_slice).unwrap_or_default();
    sessions.iter().filter_map(|s| s.get("id").and_then(Value::as_str)).map(String::from).collect()
}

/// Claims and runs each session in turn; one that fails is reported with `fail` (its id and why)
/// and the next one still runs.
async fn claim_each<C, CF, F, FF>(ids: Vec<String>, mut claim: C, mut fail: F)
where
    C: FnMut(String) -> CF,
    CF: Future<Output = Result<()>>,
    F: FnMut(String, String) -> FF,
    FF: Future<Output = ()>,
{
    for sid in ids {
        if let Err(e) = claim(sid.clone()).await {
            log::warn!("lab session {sid} failed: {e}");
            fail(sid, e.to_string()).await;
        }
    }
}

/// Marks a session FAILED on the backend, with the reason shown on the website.
async fn report_failed(session_id: String, why: String) {
    if let Err(e) = update_state(&session_id, "FAILED", Progress { message: Some(&why), ..Default::default() }).await {
        log::warn!("lab session {session_id} couldn't be marked failed on the website: {e}");
    }
}

#[cfg(test)]
mod tests {
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
        let unreadable = |_: &str| Err(Error::Invalid("server.json: expected value".into()));
        let err = picked_target(Some("esxi-1"), unreadable).unwrap_err();
        assert_eq!(err.to_string(), "server.json: expected value");
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
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
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
        let dir = t.lab(id, &[(crate::runtime::lab::COMMIT_MARKER, "ffffffffffffffffffffffffffffffffffffffff"), (".isoloom/cloud-docker/aws/main.tf", "")]);
        let aws = t.host("aws1", "aws", json!({ "useCliCreds": true }));
        server::mark_lab(&dir, Some(&aws)).unwrap();
        let state = t.data().join("deployments").join(id).join("aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}},"resources":[{}]}"#).unwrap();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        assert!(crate::runtime::lab_present(&dirs, id));
        let err = run_claimed(&test_deps(&t, Arc::default()), &claim_of(id).launch, &StartPlace::default(), false).await.unwrap_err();
        assert!(err.is_refusal() && err.to_string().contains("still running on Host aws1"), "{err}");
        assert!(!tools.called("terraform"), "nothing torn down: {:?}", tools.calls());
        assert!(state.join("terraform.tfstate").is_file());
        assert_eq!(server::marked_host(&dir).as_deref(), Some("aws1"));
    }

    #[tokio::test]
    async fn a_failed_start_of_a_lab_that_was_already_there_is_not_torn_down() {
        // A start that fails on a lab something of which was there before (here, a lab up on a
        // Proxmox host whose start this time failed for another reason) stops nothing.
        let t = crate::testing::test_app::TestApp::new();
        let id = "agent-present";
        let dir = t.lab(id, &[]);
        server::mark_lab(&dir, Some("pve1")).unwrap();
        crate::runtime::test_support::record_run(t.data(), id, Some("pve1"), "proxmox", "ready");
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let failure = Error::Invalid("the lab host didn't answer".into());
        let err = undo_start(&test_deps(&t, Arc::default()), id, Runtime::Docker, crate::runtime::lab_present(&t.dirs(), id), failure).await;
        assert_eq!(err.to_string(), "the lab host didn't answer");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert_eq!(server::marked_host(&dir).as_deref(), Some("pve1"));
    }

    #[test]
    fn only_a_start_that_brought_something_up_is_undone() {
        assert!(undoes(&Error::Invalid("unhealthy".into()), false));
        assert!(!undoes(&Error::Invalid("unhealthy".into()), true), "the lab was there before");
        assert!(!undoes(&Error::Refused("still running on AWS".into()), false), "refused before anything changed");
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
}
