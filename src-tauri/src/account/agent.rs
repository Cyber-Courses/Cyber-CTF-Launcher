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

mod confirm;

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::platform::AppDirs;
use serde_json::{Value, json};

pub use confirm::{LaunchRequest, PromptLaunch, confirm_launch};
use confirm::{confirm_here, launch_request};

use crate::account::api;
use crate::account::auth;
use crate::account::colocation;
use crate::error::{Error, Result};
use crate::labs;
use crate::platform::policy::{self, Key};
use crate::runtime::start_place::{self, StartPlace};
use crate::runtime::{Runtime, server};

/// A system notification for the player (title, body), shown when a website launch starts here.
pub type Notify = dyn Fn(&str, &str) + Send + Sync;

/// The app the background agent runs inside: its data folder, and the two things it reaches the
/// player through — a prompt to confirm a website launch and a system notification when one
/// starts here. The app shell builds it from the `AppHandle` (see `crate::app`); unit tests build
/// one with effects of their own. Keeping the effects behind closures is what lets this module
/// stay domain code, free of `tauri`.
#[derive(Clone)]
pub struct Deps {
    dirs: AppDirs,
    prompt_launch: Arc<PromptLaunch>,
    notify: Arc<Notify>,
}

impl Deps {
    /// The agent's dependencies: the data folder, the launch prompt, and the notification.
    pub fn new(dirs: AppDirs, prompt_launch: Arc<PromptLaunch>, notify: Arc<Notify>) -> Deps {
        Deps { dirs, prompt_launch, notify }
    }

    fn dirs(&self) -> &AppDirs {
        &self.dirs
    }
}

/// A stable identifier for this installation, generated once and kept in the app data
/// dir. It is the merge key for the agent, so reinstalling re-registers the same machine
/// and two machines never collide even when they share a display name.
fn install_id(dirs: &AppDirs) -> Result<String> {
    install_id_at(&dirs.data()?.join("install-id"))
}

/// The install id kept at `path`, generated and written there the first time.
fn install_id_at(path: &std::path::Path) -> Result<String> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let id = random_hex();
    crate::platform::atomic_file::write(path, &id)?;
    Ok(id)
}

/// 16 random bytes as hex: install ids, co-location tokens and nonces.
fn random_hex() -> String {
    rand::random::<[u8; 16]>().iter().fold(String::with_capacity(32), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// A human label for this machine (editable server-side later). Not an identifier.
fn machine_name() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).ok();
    machine_name_from(std::env::var("CYBERCTF_AGENT_NAME").ok(), user, std::env::consts::OS)
}

/// The machine label from an explicit name, else the user and the OS.
fn machine_name_from(explicit: Option<String>, user: Option<String>, os: &str) -> String {
    if let Some(n) = explicit
        && !n.trim().is_empty()
    {
        return n.trim().to_string();
    }
    let user = user.filter(|s| !s.trim().is_empty());
    let os = match os {
        "macos" => "Mac",
        "windows" => "PC",
        "linux" => "Linux",
        other => other,
    };
    match user {
        Some(u) => format!("{u}'s {os}"),
        None => format!("Cyber CTF {os}"),
    }
}

fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// The launcher app's own version (semver), reported so the website can tell which build each
/// machine runs. The crate version is the app's, bumped with `tauri.conf.json` and `package.json`
/// at each release (as the About menu shows it).
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// This machine's OS as the website shows it: `"macos 15.1"`, `"windows 11"`, `"linux 24.04"`, or
/// the family alone when the version can't be read.
fn os_label() -> String {
    os_label_from(std::env::consts::OS, sysinfo::System::os_version().as_deref())
}

/// The OS label from its family and the version the OS reports, lowercased; only the version's
/// first word is kept (Windows reports `"11 (22631)"`, the build in brackets).
fn os_label_from(family: &str, version: Option<&str>) -> String {
    let family = family.trim().to_lowercase();
    match version.and_then(|v| v.split_whitespace().next()) {
        Some(v) => format!("{family} {}", v.to_lowercase()),
        None => family,
    }
}

/// What this machine can run. Docker for now; VM providers are detected later.
fn capabilities() -> Vec<String> {
    vec!["docker".to_string()]
}

/// Upserts this machine as the player's launcher agent and returns the server agent id.
async fn register(dirs: &AppDirs) -> Result<String> {
    let data = api::graphql(
        "mutation ($i: ID!, $n: String!, $c: [String!]!, $a: String, $t: [LaunchTargetInput!], $v: String, $o: String) { registerLauncher(installId: $i, name: $n, capabilities: $c, arch: $a, targets: $t, version: $v, os: $o) { id } }",
        json!({ "i": install_id(dirs)?, "n": machine_name(), "c": capabilities(), "a": arch(), "t": server::launch_targets(dirs)?, "v": app_version(), "o": os_label() }),
        true,
    )
    .await?;
    data["registerLauncher"]["id"].as_str().map(String::from).ok_or_else(|| Error::Invalid("registerLauncher returned no id".into()))
}

/// Keep-alive, re-reporting the targets so hosts added since registering show on the website, and
/// the app version and OS, so an update installed since registering shows too.
async fn heartbeat(dirs: &AppDirs, agent_id: &str) -> Result<()> {
    api::graphql(
        "mutation ($id: ID!, $t: [LaunchTargetInput!], $v: String, $o: String) { launcherHeartbeat(agentId: $id, targets: $t, version: $v, os: $o) { id } }",
        json!({ "id": agent_id, "t": server::launch_targets(dirs)?, "v": app_version(), "o": os_label() }),
        true,
    )
    .await
    .map(|_| ())
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
    start_place::default_place(dirs, &l.lab_id, l.runtime, (&l.repository, &l.commit), None).await
}

/// The target picked on the website, given how to name a host (`None` when it isn't set up):
/// none when it picked none, an error when the host is gone.
fn picked_target(target: Option<&str>, host_name: impl Fn(&str) -> Result<Option<String>>) -> Result<Option<String>> {
    let Some(target) = target else { return Ok(None) };
    host_name(target)?.map(|_| Some(target.to_string())).ok_or_else(|| Error::Invalid("that host is no longer set up in the launcher".into()))
}

/// Stops a claimed lab that must not keep running under a failed session. A lab that never got
/// installed (its download failed) has nothing to stop.
async fn tear_down(dirs: &AppDirs, lab_id: &str, runtime: Runtime) -> Result<()> {
    if crate::runtime::lab_installed(dirs, lab_id) { crate::runtime::stop_lab(dirs, lab_id, runtime, |_line: String| {}).await } else { Ok(()) }
}

/// `failure`, after tearing down what the failed start brought up, when it must be (see
/// [`undoes`]). A teardown that fails too is said with it: the lab may still be running (and,
/// on a cloud account, billing), and the website and the log must say so.
async fn undo_start(dirs: &AppDirs, lab_id: &str, runtime: Runtime, present_before: bool, failure: Error) -> Error {
    if !undoes(&failure, present_before) {
        return failure;
    }
    match tear_down(dirs, lab_id, runtime).await {
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
async fn run_claimed(dirs: &AppDirs, launch: &labs::Launch, place: &StartPlace, present_before: bool) -> Result<Option<String>> {
    let placement =
        labs::Placement { host: place.host.as_deref(), provider: place.provider, per_machine_vm: place.per_machine_vm, ..labs::Placement::default() };
    match labs::run(dirs, launch.clone(), &placement, |_line: String| {}).await {
        Ok(url) => Ok(url),
        Err(e) => Err(undo_start(dirs, &launch.lab_id, launch.runtime, present_before, e).await),
    }
}

/// Tells the backend the lab runs: where its target is reachable, plus a loopback control
/// endpoint + one-time token/nonce so the website can verify co-location before trusting the
/// 127.0.0.1 URL (the endpoint ends once it answered, when the lab stops, or after a while). The relay path for a remote/headless agent is the next step.
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
    let url = run_claimed(deps.dirs(), launch, &place, present).await?;
    // The lab is actually running now. If reporting back to the backend fails, tear down what
    // this start brought up before returning the error, so we don't leave infra running under a
    // session the poller will mark FAILED.
    if let Err(e) = report_running(deps.dirs(), session_id, &launch.lab_id, host.as_deref(), url.as_deref()).await {
        return Err(undo_start(deps.dirs(), &launch.lab_id, launch.runtime, present, e).await);
    }
    // A lab launched from the website just started here: let the player know on this machine.
    (deps.notify)("Lab running", "A lab launched from the website is now running on this machine.");
    Ok(())
}

/// Picks up and runs every pending launch aimed at this agent. A failed read of them is logged;
/// the next poll asks again.
async fn poll_once(deps: &Deps, agent_id: &str) {
    match api::graphql("query ($a: ID) { myPendingLaunches(agentId: $a) { id } }", json!({ "a": agent_id }), true).await {
        Ok(data) => claim_each(pending_ids(&data), |sid| async move { claim_and_run(deps, &sid).await }, report_failed).await,
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

/// The agent this machine is registered as, and the account it was registered for.
#[derive(Clone, Debug, PartialEq)]
struct Registration {
    user: String,
    agent_id: String,
}

/// What the agent loop does next, from whom it is registered for and who is signed in now.
#[derive(Debug, PartialEq)]
enum Next {
    /// Signed out: no agent (its id is forgotten), wait for a sign-in.
    SignedOut,
    /// Signed in as someone it isn't registered for (first sign-in, or another account).
    Register,
    /// Registered for the signed-in account: heartbeat and poll.
    Run,
}

fn next_step(current: Option<&Registration>, user: Option<&str>) -> Next {
    match (current, user) {
        (_, None) => Next::SignedOut,
        (Some(r), Some(u)) if r.user == u => Next::Run,
        _ => Next::Register,
    }
}

/// The agent this machine is registered as now, for the signed-in account: none while signed
/// out, and while it registers (again, for another account). What `agent_info` says of it.
static REGISTRATION: Mutex<Option<Registration>> = Mutex::new(None);

/// The registration in `current`, poisoned or not (it is only ever replaced whole).
fn registration(current: &Mutex<Option<Registration>>) -> MutexGuard<'_, Option<Registration>> {
    current.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The current registration's agent id, if any.
fn agent_id_of(current: &Mutex<Option<Registration>>) -> Option<String> {
    registration(current).as_ref().map(|r| r.agent_id.clone())
}

/// Heartbeats (~30s) for the current registration, on its own task: claiming and running a lab
/// can block for a long time (a cloud launch waits on terraform apply, up to tens of minutes),
/// and the machine must keep reporting that it's online for the whole launch, not drop offline
/// while busy.
fn spawn_heartbeat(dirs: AppDirs, current: &'static Mutex<Option<Registration>>) {
    tokio::spawn(async move {
        loop {
            if let Some(agent_id) = agent_id_of(current)
                && auth::access_token().await.is_ok()
                && let Err(e) = heartbeat(&dirs, &agent_id).await
            {
                log::warn!("launcherHeartbeat failed: {e}");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// Runs the background agent, unless the policy turns it off: waits for login, registers, then
/// heartbeats (~30s) and polls for launches (~6s) for as long as the app runs. Signing out forgets the agent, and
/// signing in as another account registers again, so launches are never polled for (and
/// the machine never shows online for) an account that isn't signed in here. The app shell spawns
/// this on its runtime (see `crate::app`), with the effects (`Deps`) the agent reaches the player
/// through, so this loop itself is free of `tauri`.
pub async fn run(deps: Deps) {
    if policy::current().is_on(Key::DisableWebsiteAgent) {
        log::info!("the website agent is off (policy {}): this machine takes no launches from the website", Key::DisableWebsiteAgent.name());
        return;
    }
    let current = &REGISTRATION;
    spawn_heartbeat(deps.dirs().clone(), current);
    loop {
        let user = auth::signed_in_user().await;
        let registered = registration(current).clone();
        match next_step(registered.as_ref(), user.as_deref()) {
            Next::SignedOut => {
                *registration(current) = None;
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            Next::Register => {
                // Not online for the account registered before while this one registers.
                *registration(current) = None;
                // No token yet, or registering failed: wait and start the cycle over.
                if register_step(&deps, current, user).await {
                    continue;
                }
            }
            Next::Run => {}
        }
        if let Some(agent_id) = agent_id_of(current)
            && auth::access_token().await.is_ok()
        {
            Box::pin(poll_once(&deps, &agent_id)).await;
        }
        tokio::time::sleep(Duration::from_secs(6)).await;
    }
}

/// Registers this machine for `user`, keeping its agent id in `current`. Returns whether the loop
/// should wait and start over — there was no access token yet, or `registerLauncher` failed — so a
/// poll is never made for an account this machine isn't registered as.
async fn register_step(deps: &Deps, current: &Mutex<Option<Registration>>, user: Option<String>) -> bool {
    if auth::access_token().await.is_err() {
        tokio::time::sleep(Duration::from_secs(5)).await;
        return true;
    }
    match register(deps.dirs()).await {
        Ok(agent_id) => {
            log::info!("launcher agent online: {agent_id}");
            *registration(current) = Some(Registration { user: user.unwrap_or_default(), agent_id });
            false
        }
        Err(e) => {
            log::warn!("registerLauncher failed, retrying: {e}");
            tokio::time::sleep(Duration::from_secs(15)).await;
            true
        }
    }
}

/// This machine's launcher-agent identity, for the UI ("running as ...").
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    install_id: String,
    name: String,
    arch: &'static str,
    capabilities: Vec<String>,
    /// Whether the website sees this machine, as of now.
    state: AgentState,
}

/// Whether the website sees this machine as somewhere to launch labs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    /// The administrator's policy turned the agent off (`DisableWebsiteAgent`).
    OffByPolicy,
    /// Not registered (yet) for the signed-in account: waiting for its first registration, or
    /// retrying one that failed.
    Registering,
    /// Registered for the signed-in account: it shows online and takes launches.
    Online,
}

/// The agent's state, from whether the policy turns it `off` and whether it is `registered`.
fn agent_state(off: bool, registered: bool) -> AgentState {
    match (off, registered) {
        (true, _) => AgentState::OffByPolicy,
        (false, false) => AgentState::Registering,
        (false, true) => AgentState::Online,
    }
}

/// This machine's identity, for the UI ("running as ..."), and whether the website sees it.
pub fn agent_info(dirs: &AppDirs) -> Result<AgentInfo> {
    let state = agent_state(policy::current().is_on(Key::DisableWebsiteAgent), registration(&REGISTRATION).is_some());
    Ok(AgentInfo { install_id: install_id(dirs)?, name: machine_name(), arch: arch(), capabilities: capabilities(), state })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent's dependencies against a test app: its data folder, the prompts it shows kept
    /// in `prompts`, and a no-op notification (a system notification has nothing to show here).
    pub(super) fn test_deps(t: &crate::testing::test_app::TestApp, prompts: Arc<Mutex<Vec<LaunchRequest>>>) -> Deps {
        let prompt = move |request: &LaunchRequest| prompts.lock().unwrap().push(request.clone());
        Deps::new(t.dirs(), Arc::new(prompt), Arc::new(|_: &str, _: &str| {}))
    }

    #[test]
    fn the_agent_registers_again_for_another_account_and_forgets_it_on_sign_out() {
        // After signing out and in as someone else, the old agent id was still polled with.
        let a = Registration { user: "u1".into(), agent_id: "agent-1".into() };
        assert_eq!(next_step(None, None), Next::SignedOut);
        assert_eq!(next_step(None, Some("u1")), Next::Register);
        assert_eq!(next_step(Some(&a), Some("u1")), Next::Run);
        assert_eq!(next_step(Some(&a), Some("u2")), Next::Register);
        assert_eq!(next_step(Some(&a), None), Next::SignedOut);
        let current = Mutex::new(Some(a));
        assert_eq!(agent_id_of(&current).as_deref(), Some("agent-1"));
        *current.lock().unwrap() = None;
        assert_eq!(agent_id_of(&current), None);
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
    fn random_hex_is_32_hex_digits() {
        let (a, b) = (random_hex(), random_hex());
        assert!(a.len() == 32 && a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn the_install_id_is_made_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("cyberctf-install-{}", random_hex()));
        let path = dir.join("data").join("install-id");
        let first = install_id_at(&path).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(install_id_at(&path).unwrap(), first);
        std::fs::write(&path, "  kept-id \n").unwrap();
        assert_eq!(install_id_at(&path).unwrap(), "kept-id");
        std::fs::write(&path, "   ").unwrap();
        let regenerated = install_id_at(&path).unwrap();
        assert_eq!(regenerated.len(), 32);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_machine_name_prefers_the_explicit_name_then_the_user() {
        assert_eq!(machine_name_from(Some("  Lab box ".into()), Some("ada".into()), "linux"), "Lab box");
        assert_eq!(machine_name_from(Some("  ".into()), Some("ada".into()), "macos"), "ada's Mac");
        assert_eq!(machine_name_from(None, Some("bob".into()), "windows"), "bob's PC");
        assert_eq!(machine_name_from(None, Some(" ".into()), "linux"), "Cyber CTF Linux");
        assert_eq!(machine_name_from(None, None, "freebsd"), "Cyber CTF freebsd");
        assert_ne!(machine_name(), "");
        assert_eq!(capabilities(), vec!["docker".to_string()]);
        assert_eq!(arch(), std::env::consts::ARCH);
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

    #[test]
    fn the_agent_is_online_only_registered_and_never_against_the_policy() {
        // Regression: Settings showed every signed-in machine online, the policy's too.
        assert_eq!(agent_state(true, false), AgentState::OffByPolicy);
        assert_eq!(agent_state(true, true), AgentState::OffByPolicy);
        assert_eq!(agent_state(false, false), AgentState::Registering);
        assert_eq!(agent_state(false, true), AgentState::Online);
        let t = crate::testing::test_app::TestApp::new();
        let _policy = policy::Override::with(&[Key::DisableWebsiteAgent]);
        assert_eq!(serde_json::to_value(agent_info(&t.dirs()).unwrap()).unwrap()["state"], "offByPolicy");
    }

    #[test]
    fn this_machines_identity_is_its_install_id_name_and_arch() {
        let t = crate::testing::test_app::TestApp::new();
        let info = || serde_json::to_value(agent_info(&t.dirs()).unwrap()).unwrap();
        let first = info();
        assert_eq!(first["installId"].as_str().unwrap().len(), 32);
        assert_eq!(info()["installId"], first["installId"], "kept");
        let info = first;
        assert_eq!(info["arch"], arch());
        assert_eq!(info["capabilities"], json!(["docker"]));
    }

    #[test]
    fn this_machine_reports_the_app_version_and_its_os_family_and_version() {
        assert_eq!(app_version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(os_label_from("macos", Some("15.1")), "macos 15.1");
        assert_eq!(os_label_from("windows", Some("11 (22631)")), "windows 11", "the build is dropped");
        assert_eq!(os_label_from("Linux", Some("24.04")), "linux 24.04", "lowercased");
        assert_eq!(os_label_from("linux", None), "linux", "the family alone without a version");
        assert_eq!(os_label_from("linux", Some("  ")), "linux", "a blank version is none");
        assert!(os_label().starts_with(std::env::consts::OS));
    }

    #[tokio::test]
    async fn the_policy_keeps_the_agent_off() {
        // With the website agent off the loop returns at once: it never registers nor polls.
        let t = crate::testing::test_app::TestApp::new();
        let _policy = policy::Override::with(&[Key::DisableWebsiteAgent]);
        let ran = tokio::time::timeout(Duration::from_secs(5), Box::pin(run(test_deps(&t, Arc::default())))).await;
        assert!(ran.is_ok(), "the agent kept running");
    }

    #[tokio::test]
    async fn a_lab_that_was_never_installed_has_nothing_to_tear_down() {
        // A lab whose download failed: the failure isn't said to have left anything up.
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        tear_down(&dirs, "gone", Runtime::Docker).await.unwrap();
        tear_down(&dirs, "gone", Runtime::Vm).await.unwrap();
        let err = undo_start(&dirs, "gone", Runtime::Docker, false, Error::Invalid("download failed: HTTP 404".into())).await;
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
        let dir = t.lab(id, &[(".cyberctf-commit", "ffffffffffffffffffffffffffffffffffffffff"), (".isoloom/cloud-docker/aws/main.tf", "")]);
        let aws = t.host("aws1", "aws", json!({ "useCliCreds": true }));
        server::mark_lab(&dir, Some(&aws)).unwrap();
        let state = t.data().join("deployments").join(id).join("aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}},"resources":[{}]}"#).unwrap();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        assert!(crate::runtime::lab_present(&dirs, id));
        let err = run_claimed(&dirs, &claim_of(id).launch, &StartPlace::default(), false).await.unwrap_err();
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
        let err = undo_start(&t.dirs(), id, Runtime::Docker, crate::runtime::lab_present(&t.dirs(), id), failure).await;
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
