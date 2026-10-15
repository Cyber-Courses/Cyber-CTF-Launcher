//! Launcher agent: "bring your own compute". Once the player is logged in, this machine
//! registers itself with CyberBackend (keyed by a stable per-install id so several
//! machines are distinct), heartbeats so it shows as online, and polls for the player's
//! launch requests. When one arrives it claims it, runs the lab locally, and reports back.
//! See doc/architecture/LAB-LAUNCHER-CHANNEL.md.
//!
//! Phase 2a (here): presence + claim-and-run. The interactive surface (loopback
//! co-location server, relay tunnel, terminal / noVNC) is Phase 2b.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::AppHandle;
use serde_json::{Value, json};
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::oneshot;

use crate::account::api;
use crate::account::auth;
use crate::account::colocation;
use crate::error::{Error, Result};
use crate::labs;
use crate::runtime::{Runtime, server};

/// A stable identifier for this installation, generated once and kept in the app data
/// dir. It is the merge key for the agent, so reinstalling re-registers the same machine
/// and two machines never collide even when they share a display name.
fn install_id(app: &AppHandle) -> Result<String> {
    install_id_at(&app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("install-id"))
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
    (0..16).map(|_| format!("{:02x}", rand::random::<u8>())).collect()
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

/// What this machine can run. Docker for now; VM providers are detected later.
fn capabilities() -> Vec<String> {
    vec!["docker".to_string()]
}

/// Upserts this machine as the player's launcher agent and returns the server agent id.
async fn register(app: &AppHandle) -> Result<String> {
    let data = api::graphql(
        "mutation ($i: ID!, $n: String!, $c: [String!]!, $a: String, $t: [LaunchTargetInput!]) { registerLauncher(installId: $i, name: $n, capabilities: $c, arch: $a, targets: $t) { id } }",
        json!({ "i": install_id(app)?, "n": machine_name(), "c": capabilities(), "a": arch(), "t": server::launch_targets(app) }),
        true,
    )
    .await?;
    data["registerLauncher"]["id"].as_str().map(String::from).ok_or_else(|| Error::Invalid("registerLauncher returned no id".into()))
}

/// Keep-alive, re-reporting the targets so hosts added since registering show on the website.
async fn heartbeat(app: &AppHandle, agent_id: &str) -> Result<()> {
    api::graphql(
        "mutation ($id: ID!, $t: [LaunchTargetInput!]) { launcherHeartbeat(agentId: $id, targets: $t) { id } }",
        json!({ "id": agent_id, "t": server::launch_targets(app) }),
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

/// Cloud-launch confirmations awaiting the user's answer, keyed by session id. The agent inserts
/// a sender and awaits it; `confirm_launch` (from the UI) resolves it.
fn pending_confirmations() -> &'static Mutex<HashMap<String, oneshot::Sender<bool>>> {
    static P: OnceLock<Mutex<HashMap<String, oneshot::Sender<bool>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The user's answer to a cloud-launch confirmation, from the UI.
#[tauri::command]
pub fn confirm_launch(session_id: String, approve: bool) {
    if let Ok(mut map) = pending_confirmations().lock()
        && let Some(tx) = map.remove(&session_id)
    {
        let _ = tx.send(approve);
    }
}

/// Asks the user to confirm a website launch that would run on one of their cloud accounts
/// (it costs money), by emitting an event the UI shows as a prompt and waiting for the answer.
/// A timeout counts as a decline, so a missed prompt doesn't hang the session or spend money.
async fn confirm_cloud_launch(app: &AppHandle, session_id: &str, repo: &str, commit: &str, target: &str) -> bool {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut map) = pending_confirmations().lock() {
        map.insert(session_id.to_string(), tx);
    }
    let _ = app.emit("launch-confirm", json!({ "sessionId": session_id, "repository": repo, "commit": commit, "target": target }));
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_focus();
    }
    let approved = await_answer(rx, CONFIRM_WAIT).await;
    if let Ok(mut map) = pending_confirmations().lock() {
        map.remove(session_id);
    }
    approved
}

/// How long a cloud-launch prompt waits for the player.
const CONFIRM_WAIT: Duration = Duration::from_secs(180);

/// The player's answer on `rx`: only an explicit approval within `wait` counts; a decline, a
/// dropped prompt or no answer in time is a no.
async fn await_answer(rx: oneshot::Receiver<bool>, wait: Duration) -> bool {
    matches!(tokio::time::timeout(wait, rx).await, Ok(Ok(true)))
}

/// Where a claimed launch runs: the target picked on the website (one of this launcher's hosts
/// or cloud accounts); else VM labs go to the default server host and Docker labs run here.
fn launch_host(app: &AppHandle, claim: &Value) -> Result<Option<String>> {
    pick_launch_host(claim, |target| server::host_name(app, target), || server::default_host(app))
}

/// `launch_host` given how to name a host (`None` when it isn't set up) and the default host.
fn pick_launch_host(claim: &Value, host_name: impl Fn(&str) -> Option<String>, default_host: impl FnOnce() -> Option<String>) -> Result<Option<String>> {
    Ok(match claim["target"].as_str() {
        Some(target) => {
            Some(host_name(target).map(|_| target.to_string()).ok_or_else(|| Error::Invalid("that host is no longer set up in the launcher".into()))?)
        }
        // A cloud lab never runs on an account it wasn't sent to (it costs money): no target, no
        // launch (runtime::start says so).
        None => (claim["runtime"] == "VM").then(default_host).flatten(),
    })
}

/// Running on a cloud account costs money, so a website launch onto one isn't auto-run: ask
/// the user to confirm on this machine first (local and server targets still run unattended).
async fn confirm_if_cloud(app: &AppHandle, session_id: &str, claim: &Value, host: Option<&str>) -> Result<()> {
    let Some(id) = host.filter(|id| server::host_provider(app, id).is_some_and(|p| p.is_cloud())) else { return Ok(()) };
    let target = server::host_name(app, id).unwrap_or_else(|| id.to_string());
    let repo = claim["repository"].as_str().unwrap_or("");
    let commit = claim["commit"].as_str().unwrap_or("");
    if confirm_cloud_launch(app, session_id, repo, commit, &target).await {
        Ok(())
    } else {
        Err(Error::Invalid(format!("Launch on {target} was not confirmed on this machine.")))
    }
}

fn claim_runtime(claim: &Value) -> Runtime {
    match claim["runtime"].as_str() {
        Some("VM") => Runtime::Vm,
        Some("CLOUD") => Runtime::Cloud,
        _ => Runtime::Docker,
    }
}

/// The attack box a launch on a server host brings; none here (the player's own setting) and
/// none for cloud services, which the player works on from their own machine. On a host it is the
/// default image (the per-machine setting lives in the webview), pinned by digest like settings.ts's.
fn attack_image(host: Option<&str>, runtime: Runtime) -> Option<&'static str> {
    host.filter(|_| runtime != Runtime::Cloud).map(|_| crate::runtime::default_attack_image())
}

/// Stops a claimed lab that must not keep running under a failed session. A lab that never got
/// installed (its download failed) has nothing to stop.
async fn tear_down(app: &AppHandle, lab_id: Option<&str>, runtime: Runtime) -> Result<()> {
    match lab_id.filter(|id| crate::runtime::lab_installed(app, id)) {
        Some(id) => crate::runtime::stop_lab(app, id, runtime, |_line: String| {}).await,
        None => Ok(()),
    }
}

/// `failure`, after tearing down what the failed start brought up, when it must be (see
/// [`undoes`]). A teardown that fails too is said with it: the lab may still be running (and,
/// on a cloud account, billing), and the website and the log must say so.
async fn undo_start(app: &AppHandle, lab_id: Option<&str>, runtime: Runtime, present_before: bool, failure: Error) -> Error {
    if !undoes(&failure, present_before) {
        return failure;
    }
    match tear_down(app, lab_id, runtime).await {
        Ok(()) => failure,
        Err(e) => {
            log::warn!("stopping what a failed launch of {lab_id:?} started: {e}");
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
    !present_before && !matches!(failure, Error::Refused(_))
}

/// Whether something of the claimed lab exists before its start (see `runtime::lab_present`).
async fn present_before(app: &AppHandle, claim: &Value) -> bool {
    match claim["labId"].as_str() {
        Some(id) => crate::runtime::lab_present(app, id).await,
        None => false,
    }
}

/// Starts a claimed lab on `host` (None: this machine). A failure tears down what this start
/// brought up, and only that (see [`undoes`]). Where the lab is reachable here, once running.
async fn run_claimed(app: &AppHandle, claim: &Value, host: Option<&str>, runtime: Runtime, present_before: bool) -> Result<Option<String>> {
    let lab_id = claim["labId"].as_str();
    match labs::run(app, claim.clone(), None, host, attack_image(host, runtime), false, |_line: String| {}).await {
        Ok(url) => Ok(url),
        Err(e) => Err(undo_start(app, lab_id, runtime, present_before, e).await),
    }
}

/// Tells the backend the lab runs: where its target is reachable, plus a loopback control
/// endpoint + one-time token/nonce so the website can verify co-location before trusting the
/// 127.0.0.1 URL (the endpoint ends once it answered, when the lab stops, or after a while). The relay path for a remote/headless agent is the next step.
async fn report_running(app: &AppHandle, session_id: &str, lab_id: Option<&str>, host: Option<&str>, url: Option<&str>) -> Result<()> {
    let running_on = running_message(host.and_then(|h| server::host_name(app, h)));
    let token = random_hex();
    let nonce = random_hex();
    let control_url = colocation::serve(lab_id, token.clone(), nonce.clone()).await.ok().map(|port| format!("http://127.0.0.1:{port}"));
    let progress = Progress { local_url: url, control_url: control_url.as_deref(), token: Some(&token), nonce: Some(&nonce), message: Some(&running_on) };
    update_state(session_id, "RUNNING", progress).await
}

/// Where the website says the lab runs: the host's name, or this machine.
fn running_message(host_name: Option<String>) -> String {
    host_name.map_or_else(|| "Running on your machine".to_string(), |n| format!("Running on {n}"))
}

/// Claims one pending session and runs its lab on this machine.
async fn claim_and_run(app: &AppHandle, session_id: &str) -> Result<()> {
    let data = api::graphql(
        "mutation ($id: ID!) { claimLaunch(sessionId: $id) { labId runtime repository commit target env { name value } } }",
        json!({ "id": session_id }),
        true,
    )
    .await?;
    let claim = &data["claimLaunch"];
    update_state(session_id, "PULLING", Progress { message: Some("Preparing the lab on your machine"), ..Default::default() }).await?;
    let host = launch_host(app, claim)?;
    confirm_if_cloud(app, session_id, claim, host.as_deref()).await?;
    let runtime = claim_runtime(claim);
    // Anything this start brought up before failing (one unhealthy service, say) is torn down, or
    // it would keep running under a FAILED session and every later launch of it would be refused.
    let present = present_before(app, claim).await;
    let url = run_claimed(app, claim, host.as_deref(), runtime, present).await?;
    // The lab is actually running now. If reporting back to the backend fails, tear down what
    // this start brought up before returning the error, so we don't leave infra running under a
    // session the poller will mark FAILED.
    if let Err(e) = report_running(app, session_id, claim["labId"].as_str(), host.as_deref(), url.as_deref()).await {
        return Err(undo_start(app, claim["labId"].as_str(), runtime, present, e).await);
    }
    // A lab launched from the website just started here: let the player know on this machine.
    let _ = app.notification().builder().title("Lab running").body("A lab launched from the website is now running on this machine.").show();
    Ok(())
}

/// Picks up and runs every pending launch aimed at this agent.
async fn poll_once(app: &AppHandle, agent_id: &str) -> Result<()> {
    let data = api::graphql("query ($a: ID) { myPendingLaunches(agentId: $a) { id } }", json!({ "a": agent_id }), true).await?;
    claim_each(pending_ids(&data), |sid| async move { claim_and_run(app, &sid).await }, report_failed).await;
    Ok(())
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
    let _ = update_state(&session_id, "FAILED", Progress { message: Some(&why), ..Default::default() }).await;
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

/// The current registration's agent id, if any.
fn agent_id_of(current: &Mutex<Option<Registration>>) -> Option<String> {
    current.lock().ok().and_then(|r| r.as_ref().map(|r| r.agent_id.clone()))
}

/// Heartbeats (~30s) for the current registration, on its own task: claiming and running a lab
/// can block for a long time (a cloud launch waits on terraform apply, up to tens of minutes),
/// and the machine must keep reporting that it's online for the whole launch, not drop offline
/// while busy.
fn spawn_heartbeat(app: AppHandle, current: std::sync::Arc<Mutex<Option<Registration>>>) {
    tokio::spawn(async move {
        loop {
            if let Some(agent_id) = agent_id_of(&current)
                && auth::access_token().await.is_ok()
            {
                let _ = heartbeat(&app, &agent_id).await;
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// Starts the background agent: waits for login, registers, then heartbeats (~30s) and
/// polls for launches (~6s) for as long as the app runs. Signing out forgets the agent, and
/// signing in as another account registers again, so launches are never polled for (and
/// the machine never shows online for) an account that isn't signed in here.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let current: std::sync::Arc<Mutex<Option<Registration>>> = Default::default();
        spawn_heartbeat(app.clone(), current.clone());
        loop {
            let user = auth::signed_in_user().await;
            let registered = current.lock().ok().and_then(|r| r.clone());
            match next_step(registered.as_ref(), user.as_deref()) {
                Next::SignedOut => {
                    if let Ok(mut r) = current.lock() {
                        *r = None;
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                Next::Register => {
                    if auth::access_token().await.is_err() {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    match register(&app).await {
                        Ok(agent_id) => {
                            log::info!("launcher agent online: {agent_id}");
                            if let Ok(mut r) = current.lock() {
                                *r = Some(Registration { user: user.unwrap_or_default(), agent_id });
                            }
                        }
                        Err(e) => {
                            log::warn!("registerLauncher failed, retrying: {e}");
                            tokio::time::sleep(Duration::from_secs(15)).await;
                            continue;
                        }
                    }
                }
                Next::Run => {}
            }
            if let Some(agent_id) = agent_id_of(&current)
                && auth::access_token().await.is_ok()
            {
                let _ = poll_once(&app, &agent_id).await;
            }
            tokio::time::sleep(Duration::from_secs(6)).await;
        }
    });
}

/// This machine's launcher-agent identity, for the UI ("running as ...").
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    install_id: String,
    name: String,
    arch: &'static str,
    capabilities: Vec<String>,
}

/// This machine's identity, for the UI ("running as ...").
#[tauri::command]
pub fn agent_info(app: AppHandle) -> Result<AgentInfo> {
    Ok(AgentInfo { install_id: install_id(&app)?, name: machine_name(), arch: arch(), capabilities: capabilities() })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_claim_runs_as_a_vm_lab_only_when_it_says_so() {
        assert!(matches!(claim_runtime(&json!({ "runtime": "VM" })), Runtime::Vm));
        assert!(matches!(claim_runtime(&json!({ "runtime": "DOCKER" })), Runtime::Docker));
        assert!(matches!(claim_runtime(&json!({})), Runtime::Docker));
        assert!(matches!(claim_runtime(&json!({ "runtime": "CLOUD" })), Runtime::Cloud));
    }

    #[test]
    fn only_container_and_vm_launches_on_a_host_bring_the_attack_box() {
        assert_eq!(attack_image(Some("aws-1"), Runtime::Docker), Some(crate::runtime::default_attack_image()));
        assert_eq!(attack_image(Some("aws-1"), Runtime::Cloud), None);
        assert_eq!(attack_image(None, Runtime::Docker), None);
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
        assert!(!machine_name().is_empty());
        assert_eq!(capabilities(), vec!["docker".to_string()]);
        assert_eq!(arch(), std::env::consts::ARCH);
    }

    #[test]
    fn a_launch_goes_to_its_target_or_vm_labs_to_the_default_host() {
        let known = |t: &str| (t == "esxi-1").then(|| "My ESXi".to_string());
        let default = || Some("proxmox-1".to_string());
        assert_eq!(pick_launch_host(&json!({ "target": "esxi-1" }), known, default).unwrap().as_deref(), Some("esxi-1"));
        let gone = pick_launch_host(&json!({ "target": "old" }), known, default).unwrap_err();
        assert_eq!(gone.to_string(), "that host is no longer set up in the launcher");
        assert_eq!(pick_launch_host(&json!({ "runtime": "VM" }), known, default).unwrap().as_deref(), Some("proxmox-1"));
        assert_eq!(pick_launch_host(&json!({ "runtime": "VM" }), known, || None).unwrap(), None);
        assert_eq!(pick_launch_host(&json!({ "runtime": "DOCKER" }), known, default).unwrap(), None);
    }

    #[test]
    fn this_machines_identity_is_its_install_id_name_and_arch() {
        let t = crate::testing::test_app::TestApp::new();
        let info = || serde_json::to_value(agent_info(t.handle().clone()).unwrap()).unwrap();
        let first = info();
        assert_eq!(first["installId"].as_str().unwrap().len(), 32);
        assert_eq!(info()["installId"], first["installId"], "kept");
        let info = first;
        assert_eq!(info["arch"], arch());
        assert_eq!(info["capabilities"], json!(["docker"]));
    }

    #[tokio::test]
    async fn a_launch_onto_a_cloud_account_runs_only_once_confirmed_here() {
        let t = crate::testing::test_app::TestApp::new();
        let app = t.handle();
        let az = t.host("az1", "azure", json!({}));
        let claim = json!({ "target": az, "repository": "CyberCTF/web", "commit": "abc" });
        assert_eq!(launch_host(app, &claim).unwrap().as_deref(), Some("az1"));
        assert_eq!(launch_host(app, &json!({ "runtime": "VM" })).unwrap(), None, "no default server host");
        // Not a cloud (here, or no host): no question asked.
        confirm_if_cloud(app, "s-local", &claim, None).await.unwrap();
        // A cloud account: the answer from the UI decides.
        for (sid, approve) in [("s-cloud-yes", true), ("s-cloud-no", false)] {
            let answer = tokio::spawn(async move {
                loop {
                    if pending_confirmations().lock().unwrap().contains_key(sid) {
                        confirm_launch(sid.into(), approve);
                        return;
                    }
                    tokio::task::yield_now().await;
                }
            });
            let r = confirm_if_cloud(app, sid, &claim, Some(&az)).await;
            answer.await.unwrap();
            match approve {
                true => r.unwrap(),
                false => assert_eq!(r.unwrap_err().to_string(), "Launch on Host az1 was not confirmed on this machine."),
            }
        }
        // A lab that was never installed (its download failed) has nothing to tear down: the
        // failure isn't said to have left anything up.
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        tear_down(app, Some("gone"), Runtime::Docker).await.unwrap();
        tear_down(app, None, Runtime::Vm).await.unwrap();
        let err = undo_start(app, Some("gone"), Runtime::Docker, false, Error::Invalid("download failed: HTTP 404".into())).await;
        assert_eq!(err.to_string(), "download failed: HTTP 404");
        assert!(tools.calls().is_empty());
    }

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A website claim of lab `id` (Docker) at another commit than the one installed.
    fn claim_of(id: &str) -> Value {
        json!({ "labId": id, "runtime": "DOCKER", "repository": "CyberCTF/web", "commit": COMMIT, "env": [] })
    }

    #[tokio::test]
    async fn a_refused_website_launch_leaves_the_lab_running_on_the_players_account() {
        // Regression: the agent asked only whether the lab ran *here*. Up on the player's AWS
        // account and launched again from the website after a new version was published, the
        // refusal ("still running on <host>. Stop it, then start it again") was followed by a
        // teardown that ran `terraform destroy` on their lab.
        let t = crate::testing::test_app::TestApp::new();
        let app = t.handle();
        let id = "agent-refused";
        let dir = t.lab(id, &[(".cyberctf-commit", "ffffffffffffffffffffffffffffffffffffffff"), (".isoloom/cloud-docker/aws/main.tf", "")]);
        let aws = t.host("aws1", "aws", json!({ "useCliCreds": true }));
        server::mark_lab(&dir, Some(&aws)).unwrap();
        let state = t.data().join("deployments").join(id).join("aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}},"resources":[{}]}"#).unwrap();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let claim = claim_of(id);
        assert!(present_before(app, &claim).await);
        let err = run_claimed(app, &claim, None, Runtime::Docker, false).await.unwrap_err();
        assert!(matches!(err, Error::Refused(_)) && err.to_string().contains("still running on Host aws1"), "{err}");
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
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let failure = Error::Invalid("the lab host didn't answer".into());
        let err = undo_start(t.handle(), Some(id), Runtime::Docker, crate::runtime::lab_present(t.handle(), id).await, failure).await;
        assert_eq!(err.to_string(), "the lab host didn't answer");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert_eq!(server::marked_host(&dir).as_deref(), Some("pve1"));
    }

    #[tokio::test]
    async fn what_a_failed_start_brought_up_is_torn_down_and_a_failed_teardown_said() {
        let t = crate::testing::test_app::TestApp::new();
        let id = "agent-fresh";
        t.lab(id, &[]);
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let err = undo_start(t.handle(), Some(id), Runtime::Docker, false, Error::Invalid("web is unhealthy".into())).await;
        assert_eq!(err.to_string(), "web is unhealthy");
        assert!(tools.called("down --volumes"), "torn down: {:?}", tools.calls());
        drop(tools);
        // The teardown fails too: the error says the lab may still be up.
        let _tools = crate::platform::exec::fake::Tools::rules(&[("down --volumes", crate::platform::exec::fake::fail("daemon not responding"))]);
        let err = undo_start(t.handle(), Some(id), Runtime::Docker, false, Error::Invalid("web is unhealthy".into())).await.to_string();
        assert!(err.starts_with("web is unhealthy Stopping what it had started failed too") && err.contains("daemon not responding"), "{err}");
    }

    #[test]
    fn only_a_start_that_brought_something_up_is_undone() {
        assert!(undoes(&Error::Invalid("unhealthy".into()), false));
        assert!(!undoes(&Error::Invalid("unhealthy".into()), true), "the lab was there before");
        assert!(!undoes(&Error::Refused("still running on AWS".into()), false), "refused before anything changed");
    }

    #[test]
    fn confirm_launch_answers_the_waiting_prompt_once() {
        let (tx, mut rx) = oneshot::channel();
        pending_confirmations().lock().unwrap().insert("s-confirm".into(), tx);
        confirm_launch("s-other".into(), true);
        assert!(rx.try_recv().is_err());
        confirm_launch("s-confirm".into(), true);
        assert!(rx.try_recv().unwrap());
        assert!(!pending_confirmations().lock().unwrap().contains_key("s-confirm"));
        let p = Progress::default();
        assert!(p.local_url.is_none() && p.control_url.is_none() && p.token.is_none() && p.nonce.is_none() && p.message.is_none());
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
    }

    #[test]
    fn the_running_message_names_the_host_or_this_machine() {
        assert_eq!(running_message(Some("My Proxmox".into())), "Running on My Proxmox");
        assert_eq!(running_message(None), "Running on your machine");
    }

    #[test]
    fn pending_launches_are_read_by_id() {
        let data = json!({ "myPendingLaunches": [{ "id": "s-1" }, { "other": 1 }, { "id": 7 }, { "id": "s-2" }] });
        assert_eq!(pending_ids(&data), ["s-1", "s-2"]);
        assert!(pending_ids(&json!({ "myPendingLaunches": null })).is_empty());
        assert!(pending_ids(&serde_json::Value::Null).is_empty());
    }

    #[tokio::test]
    async fn a_failed_claim_is_reported_and_the_next_one_still_runs() {
        let claimed = Mutex::new(Vec::new());
        let failed = Mutex::new(Vec::new());
        let ids = vec!["s-1".to_string(), "s-2".to_string(), "s-3".to_string()];
        let claim = |sid: String| {
            claimed.lock().unwrap().push(sid.clone());
            async move { if sid == "s-2" { Err(Error::Invalid("Launch on AWS was not confirmed on this machine.".into())) } else { Ok(()) } }
        };
        let fail = |sid: String, why: String| {
            failed.lock().unwrap().push((sid, why));
            async {}
        };
        claim_each(ids, claim, fail).await;
        assert_eq!(*claimed.lock().unwrap(), ["s-1", "s-2", "s-3"]);
        assert_eq!(*failed.lock().unwrap(), [("s-2".to_string(), "Launch on AWS was not confirmed on this machine.".to_string())]);
    }

    #[tokio::test]
    async fn a_cloud_launch_runs_only_on_an_explicit_yes_in_time() {
        let (tx, rx) = oneshot::channel();
        tx.send(true).unwrap();
        assert!(await_answer(rx, Duration::from_secs(1)).await);
        let (tx, rx) = oneshot::channel();
        tx.send(false).unwrap();
        assert!(!await_answer(rx, Duration::from_secs(1)).await);
        // The prompt went away (sender dropped): a no.
        let (tx, rx) = oneshot::channel::<bool>();
        drop(tx);
        assert!(!await_answer(rx, Duration::from_secs(1)).await);
        // No answer in time: a no, so a missed prompt never spends money.
        let (_tx, rx) = oneshot::channel::<bool>();
        assert!(!await_answer(rx, Duration::from_millis(20)).await);
        assert_eq!(CONFIRM_WAIT, Duration::from_secs(180));
    }
}
