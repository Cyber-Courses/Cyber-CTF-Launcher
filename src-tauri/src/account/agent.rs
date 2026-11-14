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
/// (see `labs::stop_lab_retiring_colocation`).
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
    api::graphql_signed_in(
        "mutation ($id: ID!, $s: LabSessionState!, $u: String, $c: String, $t: String, $n: String, $m: String) { updateLabSession(sessionId: $id, state: $s, localUrl: $u, controlUrl: $c, localToken: $t, coLocationNonce: $n, message: $m) { id } }",
        update_vars(session_id, state, &p),
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
    if !crate::runtime::lab_installed(dirs, lab_id) {
        return Ok(());
    }
    // Boxed: the stop's future is large, and the operation around it adds to it.
    let stop =
        |log: crate::run_service::local::Log| Box::pin(labs::stop_lab_retiring_colocation(dirs, lab_id, runtime, (deps.end_workers)(lab_id), move |l| log(l)));
    // Told as an operation of the lab, so its page shows it stopping.
    match crate::run_service::LocalRunService::of(dirs) {
        Ok(service) => Box::pin(service.observe(lab_id, crate::runtime::Operation::Stop, Some(runtime), stop)).await,
        Err(_) => stop(std::sync::Arc::new(|_| {})).await,
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
    // Boxed: the launch's future is large, and the operation around it adds to it.
    let run = |log: crate::run_service::local::Log| Box::pin(labs::run(deps.dirs(), launch.clone(), to, move |l| log(l)));
    // Told as an operation of the lab, so its page shows it starting, with its log.
    let ran = match crate::run_service::LocalRunService::of(deps.dirs()) {
        Ok(service) => Box::pin(service.observe(&launch.lab_id, crate::runtime::Operation::Launch, Some(launch.runtime), run)).await,
        Err(_) => run(std::sync::Arc::new(|_| {})).await,
    };
    match ran {
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
    let data = api::graphql_signed_in(
        "mutation ($id: ID!) { claimLaunch(sessionId: $id) { labId runtime repository commit target env { name value } } }",
        json!({ "id": session_id }),
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
    match api::graphql_signed_in("query ($a: ID) { myPendingLaunches(agentId: $a) { id } }", json!({ "a": agent_id })).await {
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
mod tests;
